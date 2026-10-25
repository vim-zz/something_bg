//! Tunnel lifecycle management (platform-agnostic).
//! Handles starting/stopping configured commands and tracking active tunnels.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Local};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetryStrategy {
    Fixed,
    #[default]
    Exponential,
}

/// Optional per-connection policy. Retry counts exclude the initial attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub strategy: RetryStrategy,
    pub initial_delay_secs: u64,
    pub max_delay_secs: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            strategy: RetryStrategy::Exponential,
            initial_delay_secs: 5,
            max_delay_secs: 60,
        }
    }
}

impl RetryPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.initial_delay_secs == 0
            || self.max_delay_secs < self.initial_delay_secs
            || self.max_delay_secs > 86_400
        {
            return Err(
                "Retry delays must satisfy 1 <= initial_delay_secs <= max_delay_secs <= 86400"
                    .into(),
            );
        }
        Ok(())
    }

    fn delay(&self, retries_done: u32) -> Option<Duration> {
        if retries_done >= self.max_retries {
            return None;
        }
        let multiplier = match self.strategy {
            RetryStrategy::Fixed => 1,
            RetryStrategy::Exponential => 1u64.checked_shl(retries_done).unwrap_or(u64::MAX),
        };
        Some(Duration::from_secs(
            self.initial_delay_secs
                .saturating_mul(multiplier)
                .min(self.max_delay_secs),
        ))
    }
}

#[derive(Clone, Debug)]
pub struct RetryStatus {
    pub attempt: u32,
    pub max_retries: u32,
    pub next_attempt_at: Instant,
}

#[derive(Clone)]
struct PendingRetry {
    status: RetryStatus,
    failure: TunnelFailure,
}

impl RetryStatus {
    pub fn remaining_secs(&self) -> u64 {
        self.next_attempt_at
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
            .ceil() as u64
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct TunnelCommand {
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
    pub retry: Option<RetryPolicy>,
}

/// Latest failed attempt, retained in memory until clear, retry, stop, or app exit.
#[derive(Clone, Debug)]
pub struct TunnelFailure {
    /// When this process launch was attempted.
    pub started_at: DateTime<FixedOffset>,
    /// When failure was observed, with the local UTC offset at that time.
    pub failed_at: DateTime<FixedOffset>,
    pub command_line: String,
    pub env_path: String,
    pub summary: String,
    pub stdout: String,
    pub stderr: String,
}

impl TunnelFailure {
    pub fn start_time_label(&self) -> String {
        format!(
            "Started at {}",
            self.started_at.format("%Y-%m-%d %H:%M:%S %:z")
        )
    }

    pub fn failure_time_label(&self) -> String {
        format!(
            "Failed at {}",
            self.failed_at.format("%Y-%m-%d %H:%M:%S %:z")
        )
    }

    pub fn logs(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.start_time_label(),
            self.failure_time_label(),
            self.error_logs()
        )
    }

    /// Error content without metadata, for display in the diagnostics panel.
    pub fn error_logs(&self) -> String {
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
    format_process_command(&command.command, &command.args, path)
}

pub(crate) fn format_process_command(command: &str, args: &[String], path: &str) -> String {
    std::iter::once("env".to_owned())
        .chain(std::iter::once(shell_quote(&format!("PATH={path}"))))
        .chain(std::iter::once(shell_quote(command)))
        .chain(args.iter().map(|arg| shell_quote(arg)))
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

pub(crate) struct OutputCapture {
    tail: Arc<Mutex<OutputTail>>,
    finished: std::sync::mpsc::Receiver<()>,
}

impl OutputCapture {
    pub(crate) fn finish(self) -> String {
        // Never wait indefinitely for descendants that inherited the output pipe.
        let _ = self.finished.recv_timeout(Duration::from_millis(250));
        self.tail.lock().unwrap().text()
    }
}

#[cfg(unix)]
pub(crate) trait OutputPipe: Read + std::os::fd::AsRawFd + Send + 'static {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsRawFd + Send + 'static> OutputPipe for T {}
#[cfg(not(unix))]
pub(crate) trait OutputPipe: Read + Send + 'static {}
#[cfg(not(unix))]
impl<T: Read + Send + 'static> OutputPipe for T {}

pub(crate) fn capture_output(mut pipe: impl OutputPipe, done: Arc<AtomicBool>) -> OutputCapture {
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
    pending_retries: Arc<Mutex<HashMap<String, PendingRetry>>>,
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

    pub fn retry_status(&self, key: &str) -> Option<RetryStatus> {
        self.pending_retries
            .lock()
            .unwrap()
            .get(key)
            .map(|retry| retry.status.clone())
    }

    pub fn retry_count(&self) -> usize {
        self.pending_retries.lock().unwrap().len()
    }

    /// Diagnostics are available during backoff without reporting a final failure.
    pub fn failure_details(&self, key: &str) -> Option<TunnelFailure> {
        let _generations = self.generations.lock().unwrap();
        self.failures.lock().unwrap().get(key).cloned().or_else(|| {
            self.pending_retries
                .lock()
                .unwrap()
                .get(key)
                .map(|retry| retry.failure.clone())
        })
    }

    /// Cancel a pending retry while retaining the failed attempt for diagnostics.
    pub fn cancel_retry(&self, key: &str) {
        let mut generations = self.generations.lock().unwrap();
        let Some(retry) = self.pending_retries.lock().unwrap().remove(key) else {
            // A stale menu action must not stop a connection that has reconnected.
            return;
        };
        *generations.entry(key.to_owned()).or_default() += 1;
        self.active_tunnels.lock().unwrap().remove(key);
        self.active_commands.lock().unwrap().remove(key);
        self.failures
            .lock()
            .unwrap()
            .insert(key.to_owned(), retry.failure);
        // Cancellation retains the warning but does not deliver a new notification.
        self.pending_failures.lock().unwrap().remove(key);
        info!("Tunnel '{key}': automatic retry cancelled; last error retained");
    }

    fn schedule_retry(
        &self,
        key: &str,
        generation: u64,
        policy: &RetryPolicy,
        retries_done: u32,
        failure: &TunnelFailure,
    ) -> Option<Instant> {
        let delay = policy.delay(retries_done)?;
        let generations = self.generations.lock().unwrap();
        if generations.get(key) != Some(&generation)
            || !self.active_tunnels.lock().unwrap().contains(key)
        {
            return None;
        }
        let next_attempt_at = Instant::now() + delay;
        self.pending_retries.lock().unwrap().insert(
            key.to_owned(),
            PendingRetry {
                status: RetryStatus {
                    attempt: retries_done + 1,
                    max_retries: policy.max_retries,
                    next_attempt_at,
                },
                failure: failure.clone(),
            },
        );
        info!(
            "Tunnel '{key}': retry {}/{} in {}s",
            retries_done + 1,
            policy.max_retries,
            delay.as_secs()
        );
        Some(next_attempt_at)
    }

    fn wait_for_retry(&self, key: &str, generation: u64, deadline: Instant) -> bool {
        loop {
            if self.generations.lock().unwrap().get(key) != Some(&generation) {
                return false;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return true;
            }
            thread::sleep(remaining.min(Duration::from_millis(100)));
        }
    }

    pub fn failure(&self, key: &str) -> Option<TunnelFailure> {
        self.failures.lock().unwrap().get(key).cloned()
    }

    pub fn failure_count(&self) -> usize {
        self.failures.lock().unwrap().len()
    }

    /// Retry a snapshot of failures without touching healthy or idle connections.
    pub fn retry_all_failed(&self) {
        let keys: Vec<String> = self.failures.lock().unwrap().keys().cloned().collect();
        info!("Retrying {} failed connection(s)", keys.len());
        for key in keys {
            self.toggle(&key, true);
        }
    }

    /// Dismiss a saved failure without starting or stopping a connection.
    pub fn clear_failure(&self, key: &str) {
        let _generations = self.generations.lock().unwrap();
        self.failures.lock().unwrap().remove(key);
        self.pending_failures.lock().unwrap().remove(key);
    }

    /// Each current failure is delivered once. Clear/retry/stop cancels pending delivery.
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
        self.pending_retries.lock().unwrap().remove(key);
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
                let mut retries_done = 0;
                loop {
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
                    manager.pending_retries.lock().unwrap().remove(&command_key);
                    let started_at = Local::now().fixed_offset();
                    let spawned = cmd.spawn();
                    drop(generations);
                    let failure = match spawned {
                        Ok(mut child) => {
                            let done = Arc::new(AtomicBool::new(false));
                            let out = capture_output(child.stdout.take().unwrap(), done.clone());
                            let err = capture_output(child.stderr.take().unwrap(), done.clone());
                            let status = child.wait();
                            // Record observation time before draining output or UI delivery.
                            let failed_at = Local::now().fixed_offset();
                            done.store(true, Ordering::Release);
                            stdout = out.finish();
                            stderr = err.finish();
                            let summary = match status {
                                // Some commands (e.g. colima start) start a daemon and
                                // exit successfully. Preserve their active state.
                                Ok(status) if status.success() => None,
                                Ok(status) => {
                                    Some(format!("Connection command exited with {status}."))
                                }
                                Err(error) => {
                                    Some(format!("Could not wait for connection command: {error}"))
                                }
                            };
                            summary.map(|summary| (summary, failed_at))
                        }
                        Err(error) => Some((
                            format!("Could not start connection command: {error}"),
                            Local::now().fixed_offset(),
                        )),
                    };
                    if let Some((summary, failed_at)) = failure {
                        let mut failure = TunnelFailure {
                            started_at,
                            failed_at,
                            command_line: format_command_line(&command, &env_path),
                            env_path: env_path.clone(),
                            summary,
                            stdout,
                            stderr,
                        };
                        if let Some(policy) = &command.retry {
                            if let Some(deadline) = manager.schedule_retry(
                                &command_key,
                                generation,
                                policy,
                                retries_done,
                                &failure,
                            ) {
                                if !manager.wait_for_retry(&command_key, generation, deadline) {
                                    return;
                                }
                                retries_done += 1;
                                continue;
                            }
                            if policy.max_retries > 0 {
                                failure.summary.push_str(&format!(
                                    " Auto retry exhausted after {retries_done} retries."
                                ));
                            }
                        }
                        manager.record_failure(&command_key, generation, failure);
                    }
                    return;
                }
            });
        } else {
            let mut generations = self.generations.lock().unwrap();
            let waiting = self
                .pending_retries
                .lock()
                .unwrap()
                .remove(command_key)
                .is_some();
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
            if !waiting {
                if let Some(command) = command {
                    if let Err(e) = stop_command(command_key, &command) {
                        error!("{e}");
                    }
                } else {
                    warn!("No command configuration found while stopping '{command_key}'");
                }
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

        let mut waiting = HashSet::new();
        for key in &affected {
            let mut generations = self.generations.lock().unwrap();
            *generations.entry(key.clone()).or_default() += 1;
            if self.pending_retries.lock().unwrap().remove(key).is_some() {
                waiting.insert(key.clone());
            }
        }

        for key in &affected {
            let Some(active_command) = active_commands.get(key) else {
                continue;
            };
            if !waiting.contains(key)
                && let Err(e) = stop_command(key, active_command)
            {
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
            if !self.pending_retries.lock().unwrap().contains_key(key)
                && let Some(command) = active_commands.get(key)
                && let Err(e) = stop_command(key, command)
            {
                error!("{e}");
            }
        }

        // Clear all active
        active.clear();
        self.pending_retries.lock().unwrap().clear();
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
            retry: None,
        }
    }

    fn failure() -> TunnelFailure {
        TunnelFailure {
            started_at: DateTime::parse_from_rfc3339("2026-09-19T14:00:00+03:00").unwrap(),
            failed_at: DateTime::parse_from_rfc3339("2026-09-19T14:05:09+03:00").unwrap(),
            command_line: "test".into(),
            env_path: "/usr/bin:/bin".into(),
            summary: "exit 255".into(),
            stdout: "Switched context".into(),
            stderr: "Token has expired".into(),
        }
    }

    #[test]
    fn failure_logs_include_original_local_time_and_exit_status() {
        assert_eq!(
            failure().logs(),
            "Started at 2026-09-19 14:00:00 +03:00\nFailed at 2026-09-19 14:05:09 +03:00\nexit 255\n\nStandard error:\nToken has expired\n\nStandard output:\nSwitched context"
        );
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
    fn retry_delays_are_bounded_and_limit_excludes_initial_attempt() {
        let mut policy = RetryPolicy {
            max_retries: 6,
            ..RetryPolicy::default()
        };
        let delays: Vec<_> = (0..7)
            .map(|attempt| policy.delay(attempt).map(|delay| delay.as_secs()))
            .collect();
        assert_eq!(
            delays,
            vec![
                Some(5),
                Some(10),
                Some(20),
                Some(40),
                Some(60),
                Some(60),
                None
            ]
        );
        policy.strategy = RetryStrategy::Fixed;
        assert_eq!(policy.delay(5), Some(Duration::from_secs(5)));
        policy.max_retries = 0;
        assert_eq!(policy.delay(0), None);
        policy.max_retries = u32::MAX;
        policy.strategy = RetryStrategy::Exponential;
        assert_eq!(policy.delay(1000), Some(Duration::from_secs(60)));
        assert_eq!(policy.delay(u32::MAX - 1), Some(Duration::from_secs(60)));
    }

    #[test]
    fn retry_wait_keeps_connection_cancellable_without_publishing_failure() {
        let manager = active_manager();
        let policy = RetryPolicy::default();
        let deadline = manager
            .schedule_retry("test", 1, &policy, 0, &failure())
            .unwrap();
        assert!(manager.has_active_tunnels());
        assert_eq!(manager.retry_status("test").unwrap().attempt, 1);
        assert_eq!(manager.retry_count(), 1);
        assert!(manager.take_failures().is_empty());
        assert!(!manager.has_failure("test"));
        assert_eq!(manager.failure_count(), 0);
        assert_eq!(
            manager.failure_details("test").unwrap().logs(),
            failure().logs()
        );
        // A second start request cannot create a duplicate retry worker.
        manager.toggle("test", true);
        assert_eq!(manager.generations.lock().unwrap()["test"], 1);
        manager.toggle("test", false);
        assert!(!manager.wait_for_retry("test", 1, deadline));
        assert!(manager.retry_status("test").is_none());
        assert_eq!(manager.retry_count(), 0);
        assert!(manager.failure_details("test").is_none());
        assert!(!manager.has_active_tunnels());
        assert!(
            manager
                .schedule_retry("test", 1, &policy, 1, &failure())
                .is_none()
        );
        manager.record_failure("test", 1, failure());
        assert!(manager.take_failures().is_empty());
    }

    #[test]
    fn cancelling_auto_retry_keeps_failure_and_invalidates_waiting_worker() {
        let manager = active_manager();
        let original = failure();
        let policy = RetryPolicy::default();
        let deadline = manager
            .schedule_retry("test", 1, &policy, 1, &original)
            .unwrap();
        manager.cancel_retry("test");
        assert!(!manager.wait_for_retry("test", 1, deadline));
        assert!(!manager.has_active_tunnels());
        assert!(manager.active_commands.lock().unwrap().is_empty());
        assert!(manager.retry_status("test").is_none());
        assert!(manager.has_failure("test"));
        assert_eq!(manager.failure_count(), 1);
        assert_eq!(
            manager.failure_details("test").unwrap().logs(),
            original.logs()
        );
        assert!(manager.take_failures().is_empty());
        assert!(
            manager
                .schedule_retry("test", 1, &policy, 2, &original)
                .is_none()
        );
        manager.cancel_retry("test");
        assert_eq!(
            manager.failure_details("test").unwrap().logs(),
            original.logs()
        );
        manager.clear_failure("test");
        assert!(!manager.has_failure("test"));
        assert!(manager.failure_details("test").is_none());
    }

    #[test]
    fn stale_cancel_action_leaves_reconnected_tunnel_running() {
        let manager = active_manager();
        manager.cancel_retry("test");
        assert!(manager.has_active_tunnels());
        assert_eq!(manager.generations.lock().unwrap()["test"], 1);
        assert!(!manager.has_failure("test"));
    }

    #[test]
    fn removing_or_quitting_a_waiting_connection_cancels_retry() {
        for quit in [false, true] {
            let manager = active_manager();
            // No process exists during backoff, so a stop command must not be needed.
            manager
                .active_commands
                .lock()
                .unwrap()
                .get_mut("test")
                .unwrap()
                .kill_command = "/nonexistent/stop".into();
            let deadline = manager
                .schedule_retry("test", 1, &RetryPolicy::default(), 0, &failure())
                .unwrap();
            if quit {
                manager.cleanup();
            } else {
                manager.reconfigure(HashMap::new(), "/usr/bin:/bin".into());
            }
            assert!(!manager.wait_for_retry("test", 1, deadline));
            assert!(manager.retry_status("test").is_none());
            assert!(!manager.has_active_tunnels());
            assert!(manager.active_commands.lock().unwrap().is_empty());
            assert!(manager.take_failures().is_empty());
        }
    }

    #[test]
    fn stale_worker_cannot_replace_new_generation_retry_or_failure() {
        let manager = active_manager();
        manager.generations.lock().unwrap().insert("test".into(), 2);
        let policy = RetryPolicy::default();
        manager
            .schedule_retry("test", 2, &policy, 1, &failure())
            .unwrap();
        assert!(
            manager
                .schedule_retry("test", 1, &policy, 0, &failure())
                .is_none()
        );
        manager.record_failure("test", 1, failure());
        assert_eq!(manager.retry_status("test").unwrap().attempt, 2);
        assert!(manager.has_active_tunnels());
        manager.record_failure("test", 2, failure());
        assert!(manager.retry_status("test").is_none());
        assert_eq!(
            manager.failure_details("test").unwrap().logs(),
            failure().logs()
        );
        assert_eq!(manager.take_failures().len(), 1);
    }

    #[test]
    fn retry_diagnostics_follow_latest_attempt_without_interrupting_retry() {
        let manager = active_manager();
        let policy = RetryPolicy::default();
        let first = failure();
        manager
            .schedule_retry("test", 1, &policy, 0, &first)
            .unwrap();
        let mut latest = first.clone();
        latest.stderr = "New failure on second attempt".into();
        latest.started_at += chrono::Duration::seconds(10);
        latest.failed_at += chrono::Duration::seconds(10);
        manager
            .schedule_retry("test", 1, &policy, 1, &latest)
            .unwrap();
        // An old worker must not replace the diagnostics of the current retry.
        assert!(
            manager
                .schedule_retry("test", 0, &policy, 0, &first)
                .is_none()
        );
        assert_eq!(
            manager.failure_details("test").unwrap().logs(),
            latest.logs()
        );
        assert_eq!(manager.retry_status("test").unwrap().attempt, 2);
        assert!(manager.has_active_tunnels());
        assert_eq!(manager.failure_count(), 0);
        assert!(manager.take_failures().is_empty());
        manager.cleanup();
        assert!(manager.failure_details("test").is_none());
    }

    #[test]
    #[cfg(unix)]
    fn automatic_retries_stop_at_limit_and_manual_retry_resets_budget() {
        let mut cmd = command();
        cmd.command = "/nonexistent/something-bg-auto-retry-test".into();
        cmd.retry = Some(RetryPolicy {
            max_retries: 2,
            initial_delay_secs: 1,
            max_delay_secs: 1,
            ..RetryPolicy::default()
        });
        let manager = TunnelManager::new(HashMap::from([("test".into(), cmd)]), "/bin".into());
        manager.toggle("test", true);
        let first = wait_for_failure(&manager);
        assert!(first.summary.contains("exhausted after 2 retries"));
        assert!(!manager.has_active_tunnels());
        assert!(manager.retry_status("test").is_none());
        assert_eq!(manager.take_failures().len(), 1);
        manager.toggle("test", true);
        let second = wait_for_failure(&manager);
        assert!(second.summary.contains("exhausted after 2 retries"));
        assert!(second.started_at > first.failed_at);
        assert_eq!(manager.take_failures().len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn failed_process_recovers_on_retry_without_failure_notification() {
        let marker = std::env::temp_dir().join(format!(
            "something-bg-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut cmd = command();
        cmd.args = vec![
            "-c".into(),
            "if [ ! -f \"$1\" ]; then touch \"$1\"; echo temporary-error >&2; exit 1; fi; exit 0"
                .into(),
            "retry-test".into(),
            marker.to_string_lossy().into_owned(),
        ];
        cmd.retry = Some(RetryPolicy {
            initial_delay_secs: 1,
            ..RetryPolicy::default()
        });
        let manager = TunnelManager::new(
            HashMap::from([("test".into(), cmd)]),
            "/usr/bin:/bin".into(),
        );
        manager.toggle("test", true);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed_retry = false;
        while Arc::strong_count(&manager.generations) > 1 {
            observed_retry |= manager.retry_status("test").is_some();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        assert!(observed_retry);
        assert!(marker.exists());
        std::fs::remove_file(marker).unwrap();
        assert!(manager.has_active_tunnels());
        assert!(manager.retry_status("test").is_none());
        assert!(!manager.has_failure("test"));
        assert!(manager.take_failures().is_empty());
        manager.toggle("test", false);
    }

    #[test]
    #[cfg(unix)]
    fn changing_policy_during_backoff_restarts_with_new_settings() {
        let manager = active_manager();
        let old_policy = RetryPolicy::default();
        let deadline = manager
            .schedule_retry("test", 1, &old_policy, 0, &failure())
            .unwrap();
        let mut replacement = command();
        replacement.retry = Some(RetryPolicy {
            max_retries: 0,
            ..old_policy
        });
        manager.reconfigure(
            HashMap::from([("test".into(), replacement.clone())]),
            "/usr/bin:/bin".into(),
        );
        assert!(!manager.wait_for_retry("test", 1, deadline));
        assert!(manager.retry_status("test").is_none());
        assert!(manager.has_active_tunnels());
        assert!(manager.active_commands.lock().unwrap().get("test") == Some(&replacement));
        assert!(
            manager
                .schedule_retry("test", 1, &RetryPolicy::default(), 1, &failure())
                .is_none()
        );
        manager.toggle("test", false);
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
        let notifications = manager.take_failures();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].1.failed_at, failure().failed_at);
        assert_eq!(notifications[0].1.started_at, failure().started_at);
        assert!(manager.take_failures().is_empty());
        assert_eq!(
            manager.failure("test").unwrap().failed_at,
            failure().failed_at
        );
    }

    #[test]
    fn retrying_an_already_active_connection_does_not_stop_or_restart_it() {
        let manager = active_manager();
        assert!(manager.toggle("test", true));
        assert!(manager.active_tunnels.lock().unwrap().contains("test"));
        assert_eq!(manager.generations.lock().unwrap().get("test"), Some(&1));
        assert!(manager.active_commands.lock().unwrap().contains_key("test"));
    }

    #[test]
    fn bulk_retry_only_starts_failed_connections_and_cancels_old_notifications() {
        let manager = active_manager();
        for key in ["failed-a", "failed-b", "idle", "cleared"] {
            manager
                .commands_config
                .lock()
                .unwrap()
                .insert(key.into(), command());
        }
        for key in ["failed-a", "failed-b", "cleared"] {
            manager
                .failures
                .lock()
                .unwrap()
                .insert(key.into(), failure());
            manager.pending_failures.lock().unwrap().insert(key.into());
        }
        manager.clear_failure("cleared");
        assert_eq!(manager.failure_count(), 2);

        manager.retry_all_failed();

        assert_eq!(manager.failure_count(), 0);
        assert!(manager.take_failures().is_empty());
        assert_eq!(
            *manager.active_tunnels.lock().unwrap(),
            HashSet::from(["test".into(), "failed-a".into(), "failed-b".into()])
        );
        let generations = manager.generations.lock().unwrap().clone();
        assert_eq!(generations.get("test"), Some(&1));
        assert!(!generations.contains_key("idle"));
        assert!(!generations.contains_key("cleared"));

        // Repeated clicks with no failures must not restart anything.
        manager.retry_all_failed();
        assert_eq!(*manager.generations.lock().unwrap(), generations);
    }

    #[test]
    fn clearing_failure_restores_disconnected_state_and_cancels_notification() {
        let manager = active_manager();
        manager.record_failure("test", 1, failure());
        manager.clear_failure("test");

        assert!(!manager.has_failure("test"));
        assert!(manager.failure("test").is_none());
        assert!(!manager.has_active_tunnels());
        assert!(manager.active_commands.lock().unwrap().is_empty());
        assert!(manager.take_failures().is_empty());
        assert!(manager.commands_config.lock().unwrap().contains_key("test"));

        // A late report from the dismissed attempt must not restore its warning.
        manager.record_failure("test", 1, failure());
        assert!(!manager.has_failure("test"));
        assert!(manager.take_failures().is_empty());
    }

    #[test]
    fn clearing_failure_leaves_other_connections_and_failures_unchanged() {
        let manager = active_manager();
        manager
            .failures
            .lock()
            .unwrap()
            .insert("other".into(), failure());
        manager
            .pending_failures
            .lock()
            .unwrap()
            .insert("other".into());
        manager.clear_failure("missing");
        manager.clear_failure("test");
        manager.clear_failure("test");

        assert!(manager.active_tunnels.lock().unwrap().contains("test"));
        assert!(manager.active_commands.lock().unwrap().contains_key("test"));
        assert_eq!(manager.generations.lock().unwrap().get("test"), Some(&1));
        assert!(manager.has_failure("other"));
        let notifications = manager.take_failures();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].0, "other");
    }

    #[test]
    fn retrying_a_removed_connection_does_not_start_it() {
        let manager = TunnelManager::default();
        assert!(!manager.toggle("removed", true));
        assert!(manager.generations.lock().unwrap().is_empty());
        assert!(manager.active_commands.lock().unwrap().is_empty());
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
        let started_at = Local::now().fixed_offset();
        manager.toggle("test", true);
        let failure = wait_for_failure(&manager);
        assert!(failure.started_at >= started_at);
        assert!(failure.started_at <= failure.failed_at);
        assert!(failure.failed_at >= started_at);
        assert!(failure.failed_at <= Local::now().fixed_offset());
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
        let started_at = Local::now().fixed_offset();
        manager.toggle("test", true);
        let original_failure = wait_for_failure(&manager);
        assert!(original_failure.started_at >= started_at);
        assert!(original_failure.started_at <= original_failure.failed_at);
        assert!(original_failure.failed_at >= started_at);
        assert!(original_failure.failed_at <= Local::now().fixed_offset());
        assert!(original_failure.summary.contains("Could not start"));
        assert_eq!(original_failure.env_path, "/usr/bin:/bin");
        let mut replacement = command();
        replacement.args = vec!["-c".into(), "echo retry >&2; exit 2".into()];
        manager.reconfigure(
            HashMap::from([("test".into(), replacement)]),
            "/custom/bin:/usr/bin:/bin".into(),
        );
        let retried_at = Local::now().fixed_offset();
        manager.toggle("test", true);
        let failure = wait_for_failure(&manager);
        assert!(failure.started_at >= retried_at);
        assert!(failure.started_at <= failure.failed_at);
        assert!(failure.failed_at >= retried_at);
        assert!(failure.failed_at >= original_failure.failed_at);
        assert_eq!(failure.stderr, "retry\n");
        assert_eq!(failure.env_path, "/custom/bin:/usr/bin:/bin");
        assert_eq!(original_failure.env_path, "/usr/bin:/bin");
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
