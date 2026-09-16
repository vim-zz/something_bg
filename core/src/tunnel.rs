//! Tunnel lifecycle management (platform-agnostic).
//! Handles starting/stopping configured commands and tracking active tunnels.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};

#[derive(Clone, PartialEq, Eq)]
pub struct TunnelCommand {
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
}

/// Latest failed attempt, retained in memory until retry, stop, or app exit.
#[derive(Clone, Debug)]
pub struct TunnelFailure {
    pub command_line: String,
    pub summary: String,
    pub stdout: String,
    pub stderr: String,
}

impl TunnelFailure {
    pub fn logs(&self) -> String {
        format!(
            "{}\n\nStandard error:\n{}\n\nStandard output:\n{}",
            self.summary,
            if self.stderr.is_empty() {
                "(no output)"
            } else {
                &self.stderr
            },
            if self.stdout.is_empty() {
                "(no output)"
            } else {
                &self.stdout
            }
        )
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&c))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn format_command_line(command: &TunnelCommand, path: &str) -> String {
    std::iter::once("env".to_owned())
        .chain(std::iter::once(shell_quote(&format!("PATH={path}"))))
        .chain(std::iter::once(shell_quote(&command.command)))
        .chain(command.args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Default)]
struct OutputTail {
    bytes: Vec<u8>,
    truncated: bool,
}

impl OutputTail {
    fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        if self.bytes.len() > OUTPUT_LIMIT {
            self.bytes.drain(..self.bytes.len() - OUTPUT_LIMIT);
            self.truncated = true;
        }
    }

    fn text(&self) -> String {
        format!(
            "{}{}",
            if self.truncated {
                "[Earlier output truncated]\n"
            } else {
                ""
            },
            String::from_utf8_lossy(&self.bytes)
        )
    }
}

struct OutputCapture {
    tail: Arc<Mutex<OutputTail>>,
    finished: std::sync::mpsc::Receiver<()>,
}

impl OutputCapture {
    fn finish(self) -> String {
        // Never wait indefinitely for descendants that inherited the output pipe.
        let _ = self.finished.recv_timeout(Duration::from_millis(250));
        self.tail.lock().unwrap().text()
    }
}

#[cfg(unix)]
trait OutputPipe: Read + std::os::fd::AsRawFd + Send + 'static {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd + Send + 'static> OutputPipe for T {}
#[cfg(not(unix))]
trait OutputPipe: Read + Send + 'static {}
#[cfg(not(unix))]
impl<T: Read + Send + 'static> OutputPipe for T {}

fn capture_output(mut pipe: impl OutputPipe, done: Arc<AtomicBool>) -> OutputCapture {
    let tail = Arc::new(Mutex::new(OutputTail::default()));
    let output = tail.clone();
    let (finished, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        #[cfg(unix)]
        // Nonblocking reads let us close pipes inherited by daemonized children.
        unsafe {
            let fd = pipe.as_raw_fd();
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                output
                    .lock()
                    .unwrap()
                    .append(b"[Unable to capture process output]\n");
                let _ = finished.send(());
                return;
            }
        }
        let mut buffer = [0; 4096];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => output.lock().unwrap().append(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if done.load(Ordering::Acquire) {
                        break;
                    }
                    #[cfg(unix)]
                    unsafe {
                        let mut descriptor = libc::pollfd {
                            fd: pipe.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        libc::poll(&mut descriptor, 1, 100);
                    }
                    #[cfg(not(unix))]
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    output
                        .lock()
                        .unwrap()
                        .append(format!("\n[Output capture failed: {error}]\n").as_bytes());
                    break;
                }
            }
        }
        let _ = finished.send(());
    });
    OutputCapture {
        tail,
        finished: receiver,
    }
}

/// Manages the lifecycle of tunnels (start, stop, cleanup).
/// Replaces the global static variables with owned fields.
#[derive(Clone, Default)]
pub struct TunnelManager {
    pub commands_config: Arc<Mutex<HashMap<String, TunnelCommand>>>,
    pub active_tunnels: Arc<Mutex<HashSet<String>>>,
    pub active_commands: Arc<Mutex<HashMap<String, TunnelCommand>>>,
    pub generations: Arc<Mutex<HashMap<String, u64>>>,
    pub env_path: Arc<Mutex<String>>,
    failures: Arc<Mutex<HashMap<String, TunnelFailure>>>,
    pending_failures: Arc<Mutex<HashSet<String>>>,
}

fn stop_command(key: &str, command: &TunnelCommand) -> Result<(), String> {
    info!("Stopping command: {} {:?}", command.command, command.args);
    let mut child = Command::new(&command.kill_command)
        .args(&command.kill_args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start stop command for tunnel '{key}': {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                debug!("Tunnel '{key}' stopped successfully");
                return Ok(());
            }
            Ok(Some(status)) => {
                return Err(format!(
                    "Stop command for tunnel '{key}' exited with status {status}"
                ));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Stop command for tunnel '{key}' timed out"));
            }
            Err(e) => {
                return Err(format!(
                    "Failed to wait for tunnel '{key}' stop command: {e}"
                ));
            }
        }
    }
}

impl TunnelManager {
    pub fn new(commands: HashMap<String, TunnelCommand>, env_path: String) -> Self {
        Self {
            commands_config: Arc::new(Mutex::new(commands)),
            env_path: Arc::new(Mutex::new(env_path)),
            ..Self::default()
        }
    }

    pub fn has_failure(&self, key: &str) -> bool {
        self.failures.lock().unwrap().contains_key(key)
    }

    pub fn failure(&self, key: &str) -> Option<TunnelFailure> {
        self.failures.lock().unwrap().get(key).cloned()
    }

    /// Each current failure is delivered once. Retry/stop cancels pending delivery.
    pub fn take_failures(&self) -> Vec<(String, TunnelFailure)> {
        let _generations = self.generations.lock().unwrap();
        let failures = self.failures.lock().unwrap();
        self.pending_failures
            .lock()
            .unwrap()
            .drain()
            .filter_map(|key| failures.get(&key).cloned().map(|failure| (key, failure)))
            .collect()
    }

    fn record_failure(&self, key: &str, generation: u64, failure: TunnelFailure) {
        let generations = self.generations.lock().unwrap();
        if generations.get(key) != Some(&generation)
            || !self.active_tunnels.lock().unwrap().remove(key)
        {
            return;
        }
        error!("Tunnel '{key}': {}", failure.summary);
        self.active_commands.lock().unwrap().remove(key);
        self.failures
            .lock()
            .unwrap()
            .insert(key.to_owned(), failure);
        self.pending_failures.lock().unwrap().insert(key.to_owned());
    }

    /// Toggles a tunnel by name (command_key) on or off.
    /// If turning on, spawns a thread to run the SSH command.
    /// If turning off, kills the process.
    /// Toggle a tunnel on/off. Returns `true` if any tunnels are active after the toggle.
    pub fn toggle(&self, command_key: &str, enable: bool) -> bool {
        if enable {
            let command = {
                let config = self.commands_config.lock().unwrap();
                config.get(command_key).cloned()
            };
            let Some(command) = command else {
                warn!("No command configuration found while starting '{command_key}'");
                return self.has_active_tunnels();
            };

            // Serialize lifecycle changes with completion, so an old process cannot
            // clear a newer connection or report an intentional stop as a fault.
            let mut generations = self.generations.lock().unwrap();
            if self.active_tunnels.lock().unwrap().contains(command_key) {
                return self.has_active_tunnels();
            }
            let generation = generations.entry(command_key.to_owned()).or_default();
            *generation += 1;
            let generation = *generation;
            self.failures.lock().unwrap().remove(command_key);
            self.pending_failures.lock().unwrap().remove(command_key);
            self.active_tunnels
                .lock()
                .unwrap()
                .insert(command_key.to_owned());
            self.active_commands
                .lock()
                .unwrap()
                .insert(command_key.to_owned(), command.clone());
            drop(generations);

            let manager = self.clone();
            let command_key = command_key.to_owned();
            let env_path = self.env_path.lock().unwrap().clone();
            thread::spawn(move || {
                let mut cmd = Command::new(&command.command);
                cmd.args(&command.args)
                    .env("PATH", &env_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut stdout = String::new();
                let mut stderr = String::new();
                let generations = manager.generations.lock().unwrap();
                if generations.get(&command_key) != Some(&generation) {
                    return;
                }
                let spawned = cmd.spawn();
                drop(generations);
                let failure = match spawned {
                    Ok(mut child) => {
                        let done = Arc::new(AtomicBool::new(false));
                        let out = capture_output(child.stdout.take().unwrap(), done.clone());
                        let err = capture_output(child.stderr.take().unwrap(), done.clone());
                        let status = child.wait();
                        done.store(true, Ordering::Release);
                        stdout = out.finish();
                        stderr = err.finish();
                        match status {
                            // Some commands (e.g. colima start) start a daemon and
                            // exit successfully. Preserve their active state.
                            Ok(status) if status.success() => None,
                            Ok(status) => Some(format!("Connection command exited with {status}.")),
                            Err(error) => {
                                Some(format!("Could not wait for connection command: {error}"))
                            }
                        }
                    }
                    Err(error) => Some(format!("Could not start connection command: {error}")),
                };
                if let Some(summary) = failure {
                    manager.record_failure(
                        &command_key,
                        generation,
                        TunnelFailure {
                            command_line: format_command_line(&command, &env_path),
                            summary,
                            stdout,
                            stderr,
                        },
                    );
                }
            });
        } else {
            let mut generations = self.generations.lock().unwrap();
            self.active_tunnels.lock().unwrap().remove(command_key);
            *generations.entry(command_key.to_owned()).or_default() += 1;
            self.failures.lock().unwrap().remove(command_key);
            self.pending_failures.lock().unwrap().remove(command_key);

            let command = self
                .active_commands
                .lock()
                .unwrap()
                .remove(command_key)
                .or_else(|| {
                    self.commands_config
                        .lock()
                        .unwrap()
                        .get(command_key)
                        .cloned()
                });

            drop(generations);
            if let Some(command) = command {
                if let Err(e) = stop_command(command_key, &command) {
                    error!("{e}");
                }
            } else {
                warn!("No command configuration found while stopping '{command_key}'");
            }
        }

        self.has_active_tunnels()
    }

    /// Apply new definitions, restarting only active tunnels affected by the change.
    pub fn reconfigure(&self, commands: HashMap<String, TunnelCommand>, env_path: String) {
        let path_changed = *self.env_path.lock().unwrap() != env_path;
        let active_commands = self.active_commands.lock().unwrap().clone();
        let affected: Vec<String> = active_commands
            .iter()
            .filter(|(key, active_command)| {
                path_changed || commands.get(*key) != Some(*active_command)
            })
            .map(|(key, _)| key.clone())
            .collect();

        for key in &affected {
            let mut generations = self.generations.lock().unwrap();
            *generations.entry(key.clone()).or_default() += 1;
        }

        for key in &affected {
            let Some(active_command) = active_commands.get(key) else {
                continue;
            };
            if let Err(e) = stop_command(key, active_command) {
                error!("Config reload could not restart tunnel '{key}': {e}");
                continue;
            }
            self.active_tunnels.lock().unwrap().remove(key);
            self.active_commands.lock().unwrap().remove(key);
        }

        self.failures
            .lock()
            .unwrap()
            .retain(|key, _| commands.contains_key(key));
        *self.commands_config.lock().unwrap() = commands;
        *self.env_path.lock().unwrap() = env_path;

        for key in affected {
            if !self.active_tunnels.lock().unwrap().contains(&key)
                && self.commands_config.lock().unwrap().contains_key(&key)
            {
                self.toggle(&key, true);
            }
        }
    }

    /// Cleans up all tunnels when the app terminates.
    pub fn cleanup(&self) {
        let mut generations = self.generations.lock().unwrap();
        for generation in generations.values_mut() {
            *generation += 1;
        }
        let active_commands = self.active_commands.lock().unwrap().clone();
        let mut active = self.active_tunnels.lock().unwrap();

        for key in active.iter() {
            debug!("Cleaning up tunnel: {}", key);
            if let Some(command) = active_commands.get(key)
                && let Err(e) = stop_command(key, command)
            {
                error!("{e}");
            }
        }

        // Clear all active
        active.clear();
        self.active_commands.lock().unwrap().clear();
        debug!("All tunnels cleaned up");
    }

    /// Restart all tunnels currently marked as active. Useful after system wake.
    pub fn restart_active_tunnels(&self) {
        // Snapshot active tunnel keys to avoid holding the lock while restarting.
        let active_keys: Vec<String> = {
            let tunnels = self.active_tunnels.lock().unwrap();
            tunnels.iter().cloned().collect()
        };

        if active_keys.is_empty() {
            info!("No active tunnels to restart after wake");
            return;
        }

        info!(
            "Restarting {} active tunnel(s) after wake: {:?}",
            active_keys.len(),
            active_keys
        );

        for key in active_keys {
            // Stop then start each tunnel to re-establish connections after sleep.
            let _ = self.toggle(&key, false);
            thread::sleep(Duration::from_millis(150));
            let _ = self.toggle(&key, true);
        }
    }
}

impl TunnelManager {
    pub fn has_active_tunnels(&self) -> bool {
        let tunnels = self.active_tunnels.lock().unwrap();
        !tunnels.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> TunnelCommand {
        TunnelCommand {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "exit 0".into()],
            kill_command: "/usr/bin/true".into(),
            kill_args: vec![],
        }
    }

    fn failure() -> TunnelFailure {
        TunnelFailure {
            command_line: "test".into(),
            summary: "exit 255".into(),
            stdout: "Switched context".into(),
            stderr: "Token has expired".into(),
        }
    }

    fn active_manager() -> TunnelManager {
        let manager = TunnelManager::new(
            HashMap::from([("test".into(), command())]),
            "/usr/bin:/bin".into(),
        );
        manager.generations.lock().unwrap().insert("test".into(), 1);
        manager.active_tunnels.lock().unwrap().insert("test".into());
        manager
            .active_commands
            .lock()
            .unwrap()
            .insert("test".into(), command());
        manager
    }

    #[test]
    fn failure_clears_active_state_and_notifies_once() {
        let manager = active_manager();
        manager.record_failure("test", 1, failure());
        assert!(!manager.has_active_tunnels());
        assert!(manager.active_commands.lock().unwrap().is_empty());
        assert!(
            manager
                .failure("test")
                .unwrap()
                .logs()
                .contains("Token has expired")
        );
        assert_eq!(manager.take_failures().len(), 1);
        assert!(manager.take_failures().is_empty());
        assert!(manager.failure("test").is_some());
    }

    #[test]
    fn old_failure_cannot_overwrite_new_connection() {
        let manager = active_manager();
        manager.generations.lock().unwrap().insert("test".into(), 2);
        manager.record_failure("test", 1, failure());
        assert!(manager.has_active_tunnels());
        assert!(manager.failure("test").is_none());
        assert!(manager.take_failures().is_empty());
    }

    #[test]
    fn intentional_stop_does_not_become_faulty() {
        let manager = active_manager();
        manager.active_tunnels.lock().unwrap().clear();
        manager.record_failure("test", 1, failure());
        assert!(manager.failure("test").is_none());
        assert!(manager.take_failures().is_empty());
    }

    #[test]
    fn removed_configuration_discards_failure() {
        let manager = active_manager();
        manager.record_failure("test", 1, failure());
        manager.reconfigure(HashMap::new(), "/usr/bin:/bin".into());
        assert!(manager.failure("test").is_none());
        assert!(manager.take_failures().is_empty());
    }

    #[test]
    fn output_tail_is_bounded_and_marks_truncation() {
        let mut tail = OutputTail::default();
        tail.append(&vec![b'x'; OUTPUT_LIMIT]);
        tail.append(b"final error\xff");
        assert_eq!(tail.bytes.len(), OUTPUT_LIMIT);
        assert!(tail.text().starts_with("[Earlier output truncated]"));
        assert!(tail.text().ends_with("final error\u{fffd}"));
    }

    #[test]
    fn command_quotes_shell_metacharacters_and_empty_arguments() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
        assert_eq!(shell_quote("$HOME; `whoami`"), "'$HOME; `whoami`'");
        let mut cmd = command();
        cmd.args = vec![
            "-c".into(),
            "kubectl config use-context prod && kubectl port-forward svc/api 3000:3000".into(),
        ];
        assert_eq!(
            format_command_line(&cmd, "/bin:/test path"),
            "env 'PATH=/bin:/test path' /bin/sh -c 'kubectl config use-context prod && kubectl port-forward svc/api 3000:3000'"
        );
    }

    #[cfg(unix)]
    fn wait_for_failure(manager: &TunnelManager) -> TunnelFailure {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(failure) = manager.failure("test") {
                return failure;
            }
            assert!(
                Instant::now() < deadline,
                "Connection did not report its failure"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[cfg(unix)]
    fn failed_process_captures_both_streams_and_exit_code() {
        let mut cmd = command();
        cmd.args = vec!["-c".into(), "printf 'Switched to context prod\\n'; printf 'aws: Token has expired\\n' >&2; exit 255".into()];
        let manager = TunnelManager::new(
            HashMap::from([("test".into(), cmd)]),
            "/usr/bin:/bin".into(),
        );
        manager.toggle("test", true);
        let failure = wait_for_failure(&manager);
        assert!(failure.summary.contains("255"));
        assert_eq!(failure.stdout, "Switched to context prod\n");
        assert_eq!(failure.stderr, "aws: Token has expired\n");
        assert!(!manager.has_active_tunnels());
    }

    #[test]
    #[cfg(unix)]
    fn missing_executable_reports_failure_and_retry_replaces_it() {
        let mut cmd = command();
        cmd.command = "/nonexistent/something-bg-test".into();
        let manager = TunnelManager::new(
            HashMap::from([("test".into(), cmd)]),
            "/usr/bin:/bin".into(),
        );
        manager.toggle("test", true);
        assert!(
            wait_for_failure(&manager)
                .summary
                .contains("Could not start")
        );
        let mut replacement = command();
        replacement.args = vec!["-c".into(), "echo retry >&2; exit 2".into()];
        manager.reconfigure(
            HashMap::from([("test".into(), replacement)]),
            "/usr/bin:/bin".into(),
        );
        manager.toggle("test", true);
        let failure = wait_for_failure(&manager);
        assert_eq!(failure.stderr, "retry\n");
        assert_eq!(manager.take_failures().len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn successful_service_start_remains_active_without_retrying() {
        let manager = TunnelManager::new(
            HashMap::from([("test".into(), command())]),
            "/usr/bin:/bin".into(),
        );
        manager.toggle("test", true);
        let deadline = Instant::now() + Duration::from_secs(5);
        // The worker owns one manager clone until it has reaped the child and
        // finished capturing output, even when no failure event is produced.
        while Arc::strong_count(&manager.generations) > 1 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        assert!(manager.has_active_tunnels());
        assert!(manager.failure("test").is_none());
        assert!(manager.take_failures().is_empty());
        assert_eq!(manager.generations.lock().unwrap()["test"], 1);
        manager.toggle("test", false);
        assert!(!manager.has_active_tunnels());
    }

    #[test]
    #[cfg(unix)]
    fn large_output_does_not_block_failure_detection() {
        let mut cmd = command();
        cmd.args = vec![
            "-c".into(),
            "head -c 200000 /dev/zero; head -c 200000 /dev/zero >&2; echo final-error >&2; exit 1"
                .into(),
        ];
        let manager = TunnelManager::new(
            HashMap::from([("test".into(), cmd)]),
            "/usr/bin:/bin".into(),
        );
        manager.toggle("test", true);
        let failure = wait_for_failure(&manager);
        assert!(failure.stdout.starts_with("[Earlier output truncated]"));
        assert!(failure.stderr.ends_with("final-error\n"));
    }
}
