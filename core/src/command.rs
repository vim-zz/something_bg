//! One-time command runner with configurable output modes.
//!
//! Each command can run in one of three modes:
//! - **Silent**: run in background, notify only on failure
//! - **Notify**: run in background, capture output, send notification on completion
//! - **Terminal**: open a terminal emulator and execute the command there

use chrono::{DateTime, FixedOffset, Local};
use log::{error, info, warn};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use crate::config::CommandConfig;
use crate::tunnel::{TunnelFailure, capture_output, format_process_command};

/// How to handle command output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Silent,
    Notify,
    Terminal,
}

impl OutputMode {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s.map(|s| s.to_lowercase()).as_deref() {
            Some("notify") => Self::Notify,
            Some("terminal") => Self::Terminal,
            _ => Self::Silent,
        }
    }
}

struct CommandEntry {
    name: String,
    command: String,
    args: Vec<String>,
    output_mode: OutputMode,
}

/// Event passed to the notify callback with structured data.
pub struct NotifyEvent<'a> {
    pub name: &'a str,
    pub success: bool,
    pub output: &'a str,
    pub elapsed: Option<std::time::Duration>,
    pub is_running: bool,
    /// Full diagnostic snapshot for failed background commands and scheduled tasks.
    pub failure: Option<&'a TunnelFailure>,
}

/// Reports failures in all background modes, plus progress/success in notify mode.
pub type NotifyCallback = Arc<dyn Fn(&NotifyEvent) + Send + Sync>;

/// Callback invoked for "terminal" mode.
/// Parameters: (command, args)
pub type TerminalCallback = Arc<dyn Fn(&str, &[String]) + Send + Sync>;

/// Runs one-time commands with configurable output handling.
pub struct CommandRunner {
    env_path: String,
    commands: HashMap<String, CommandEntry>,
    reporter: CommandReporter,
    terminal_cb: Option<TerminalCallback>,
}

impl CommandRunner {
    pub fn new(env_path: String) -> Self {
        Self {
            env_path,
            commands: HashMap::new(),
            reporter: CommandReporter::default(),
            terminal_cb: None,
        }
    }

    pub fn set_history_path(&mut self, path: PathBuf) {
        self.reporter.history_path = Some(path);
    }

    /// Return the history log path, if configured.
    pub fn history_path(&self) -> Option<&std::path::Path> {
        self.reporter.history_path.as_deref()
    }

    pub fn set_notify_callback(&mut self, cb: NotifyCallback) {
        self.reporter.notify_cb = Some(cb);
    }

    /// Share notification delivery and history with the task scheduler.
    pub fn reporter(&self) -> CommandReporter {
        self.reporter.clone()
    }

    pub fn set_terminal_callback(&mut self, cb: TerminalCallback) {
        self.terminal_cb = Some(cb);
    }

    /// Register a command from a config entry.
    pub fn add_from_config(&mut self, key: String, config: &CommandConfig) {
        self.commands.insert(
            key,
            CommandEntry {
                name: config.name.clone(),
                command: config.command.clone(),
                args: config.args.clone(),
                output_mode: OutputMode::from_str_opt(config.output.as_deref()),
            },
        );
    }

    /// Register all commands from a config slice.
    pub fn register_all(&mut self, commands: &[(String, CommandConfig)]) {
        for (key, cmd_config) in commands {
            self.add_from_config(key.clone(), cmd_config);
        }
    }

    /// Replace all configured commands and the PATH used to execute them.
    pub fn reconfigure(&mut self, env_path: String, commands: &[(String, CommandConfig)]) {
        self.env_path = env_path;
        self.commands.clear();
        self.register_all(commands);
    }

    /// Run a registered command by its key.
    pub fn run_by_key(&self, key: &str) -> Result<(), String> {
        let entry = self
            .commands
            .get(key)
            .ok_or_else(|| format!("Unknown command key: {}", key))?;

        info!(
            "Running command '{}' ({}) in {:?} mode",
            entry.name, key, entry.output_mode
        );

        match entry.output_mode {
            OutputMode::Silent | OutputMode::Notify => self.reporter.run_background(
                &entry.name,
                &entry.command,
                &entry.args,
                &self.env_path,
                entry.output_mode == OutputMode::Notify,
            ),
            OutputMode::Terminal => self.run_terminal(entry),
        }
    }

    fn run_terminal(&self, entry: &CommandEntry) -> Result<(), String> {
        if let Some(cb) = &self.terminal_cb {
            cb(&entry.command, &entry.args);
            append_history(
                &self.reporter.history_path,
                &entry.name,
                "STARTED",
                "(opened in terminal; completion is not monitored)",
                Local::now().fixed_offset(),
            );
            Ok(())
        } else {
            Err("No terminal callback configured".to_string())
        }
    }
}

/// Shared reporting for one-shot commands and scheduled tasks.
#[derive(Clone, Default)]
pub struct CommandReporter {
    notify_cb: Option<NotifyCallback>,
    history_path: Option<PathBuf>,
}

impl CommandReporter {
    /// Start a monitored process without blocking the UI or scheduler on completion.
    pub(crate) fn run_background(
        &self,
        name: &str,
        command: &str,
        args: &[String],
        env_path: &str,
        notify_success: bool,
    ) -> Result<(), String> {
        let started = std::time::Instant::now();
        let command_line = format_process_command(command, args, env_path);
        let started_at = Local::now().fixed_offset();
        let child = Command::new(command)
            .args(args)
            .env("PATH", env_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                let failure = TunnelFailure {
                    started_at,
                    failed_at: Local::now().fixed_offset(),
                    command_line,
                    env_path: env_path.to_owned(),
                    summary: format!("Could not start command: {error}"),
                    stdout: String::new(),
                    stderr: String::new(),
                };
                self.report_failure(name, &failure, started.elapsed());
                return Err(failure.summary);
            }
        };
        let reporter = self.clone();
        let name = name.to_owned();
        let env_path = env_path.to_owned();
        thread::spawn(move || {
            // Serialize the progress callback with completion to avoid a late
            // "Running" notification arriving after the result.
            let completed = Arc::new(std::sync::Mutex::new(false));
            if notify_success && let Some(cb) = reporter.notify_cb.clone() {
                let completed = completed.clone();
                let name = name.clone();
                thread::spawn(move || {
                    thread::sleep(std::time::Duration::from_secs(2));
                    let completed = completed.lock().unwrap();
                    if !*completed {
                        cb(&NotifyEvent {
                            name: &name,
                            success: true,
                            output: "",
                            elapsed: None,
                            is_running: true,
                            failure: None,
                        });
                    }
                });
            }
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let out = capture_output(child.stdout.take().unwrap(), done.clone());
            let err = capture_output(child.stderr.take().unwrap(), done.clone());
            let status = child.wait();
            let finished_at = Local::now().fixed_offset();
            let elapsed = started.elapsed();
            *completed.lock().unwrap() = true;
            done.store(true, std::sync::atomic::Ordering::Release);
            let stdout = out.finish();
            let stderr = err.finish();
            let failure_summary = match status {
                Ok(status) if status.success() => None,
                Ok(status) => Some(format!("Command exited with {status}.")),
                Err(error) => Some(format!("Could not wait for command: {error}")),
            };
            if let Some(summary) = failure_summary {
                reporter.report_failure(
                    &name,
                    &TunnelFailure {
                        started_at,
                        failed_at: finished_at,
                        command_line,
                        env_path,
                        summary,
                        stdout,
                        stderr,
                    },
                    elapsed,
                );
            } else {
                let combined = combined_output(&stdout, &stderr);
                append_history(
                    &reporter.history_path,
                    &name,
                    "OK",
                    &format!(
                        "Command: {command_line}\nExit code: 0 (took {})\n{combined}",
                        format_duration(elapsed),
                    ),
                    finished_at,
                );
                if notify_success && let Some(cb) = &reporter.notify_cb {
                    cb(&NotifyEvent {
                        name: &name,
                        success: true,
                        output: &last_lines(&combined),
                        elapsed: Some(elapsed),
                        is_running: false,
                        failure: None,
                    });
                }
            }
        });
        Ok(())
    }

    fn report_failure(&self, name: &str, failure: &TunnelFailure, elapsed: std::time::Duration) {
        error!("Command '{name}': {}", failure.summary);
        append_history(
            &self.history_path,
            name,
            "FAILED",
            &format!(
                "Command: {}\nPATH: {}\n{}\nDuration: {}",
                failure.command_line,
                failure.env_path,
                failure.logs(),
                format_duration(elapsed),
            ),
            failure.failed_at,
        );
        if let Some(cb) = &self.notify_cb {
            let output = format!(
                "{}\n{}\n{}",
                failure.failure_time_label(),
                failure.summary,
                last_lines(&combined_output(&failure.stdout, &failure.stderr))
            );
            cb(&NotifyEvent {
                name,
                success: false,
                output: &output,
                elapsed: Some(elapsed),
                is_running: false,
                failure: Some(failure),
            });
        }
    }
}

fn combined_output(stdout: &str, stderr: &str) -> String {
    match (stdout.is_empty(), stderr.is_empty()) {
        (_, true) => stdout.to_owned(),
        (true, _) => stderr.to_owned(),
        _ => format!("{stdout}\n{stderr}"),
    }
}

fn last_lines(output: &str) -> String {
    let lines: Vec<_> = output.lines().collect();
    lines[lines.len().saturating_sub(5)..].join("\n")
}

/// Append a timestamped entry to the command history log.
fn append_history(
    path: &Option<PathBuf>,
    name: &str,
    status: &str,
    output: &str,
    finished_at: DateTime<FixedOffset>,
) {
    let Some(path) = path else { return };

    let timestamp = finished_at.format("%Y-%m-%d %H:%M:%S %:z");
    let entry = format!("=== [{timestamp}] {name} [{status}] ===\n{output}\n\n");

    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(entry.as_bytes()) {
                warn!("Failed to write command history: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to open command history file {:?}: {}", path, e);
        }
    }
}

/// Format a duration as a human-readable string (e.g. "14s", "2m 30s", "1h 5m").
pub fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{}m", m)
        } else {
            format!("{}m {}s", m, s)
        }
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{}h", h)
        } else {
            format!("{}h {}m", h, m)
        }
    }
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

    pub struct Completion {
        pub success: bool,
        pub output: String,
        pub failure: Option<TunnelFailure>,
    }

    pub struct Harness {
        pub runner: CommandRunner,
        pub events: std::sync::Mutex<Receiver<Completion>>,
        root: PathBuf,
    }

    impl Harness {
        pub fn new(script: &str, mode: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "something-bg-reports-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let mut runner = CommandRunner::new("/usr/bin:/bin".into());
            runner.set_history_path(root.join("command_history.log"));
            let (sender, events) = mpsc::channel();
            runner.set_notify_callback(Arc::new(move |event| {
                if !event.is_running {
                    sender
                        .send(Completion {
                            success: event.success,
                            output: event.output.to_owned(),
                            failure: event.failure.cloned(),
                        })
                        .unwrap();
                }
            }));
            runner.add_from_config(
                "test".into(),
                &CommandConfig {
                    name: "Test job".into(),
                    command: "/bin/sh".into(),
                    args: vec!["-c".into(), script.into()],
                    output: Some(mode.into()),
                },
            );
            Self {
                runner,
                events: std::sync::Mutex::new(events),
                root,
            }
        }

        pub fn result(&self) -> Completion {
            self.events
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
        }

        pub fn history(&self) -> String {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(history) = std::fs::read_to_string(self.root.join("command_history.log"))
                    && !history.is_empty()
                {
                    return history;
                }
                assert!(Instant::now() < deadline, "Missing completion history");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl crate::platform::AppPaths for Harness {
        fn config_path(&self) -> PathBuf {
            self.root.join("config.toml")
        }
        fn state_path(&self) -> PathBuf {
            self.root.join("task_state.toml")
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn silent_and_notify_failures_report_time_status_and_both_streams() {
        for mode in ["silent", "notify"] {
            let harness = Harness::new("printf 'context'; printf 'bad token' >&2; exit 23", mode);
            let started_at = Local::now().fixed_offset();
            harness.runner.run_by_key("test").unwrap();
            let result = harness.result();
            assert!(!result.success);
            let failure = result.failure.unwrap();
            assert!(failure.started_at >= started_at);
            assert!(failure.started_at <= failure.failed_at);
            assert!(failure.failed_at >= started_at);
            assert!(failure.failed_at <= Local::now().fixed_offset());
            assert!(failure.summary.contains("23"));
            assert_eq!(failure.stdout, "context");
            assert_eq!(failure.stderr, "bad token");
            assert_eq!(failure.env_path, "/usr/bin:/bin");
            assert!(result.output.contains(&failure.failure_time_label()));
            let history = harness.history();
            assert!(history.contains("[FAILED]"));
            assert!(history.contains(&failure.logs()));
            assert!(history.contains(&failure.command_line));
            assert!(!history.contains("[OK]"));
            assert!(harness.events.lock().unwrap().try_recv().is_err());
        }
    }

    #[test]
    fn launch_errors_report_once_in_both_background_modes() {
        for mode in ["silent", "notify"] {
            let mut harness = Harness::new("", mode);
            harness.runner.commands.get_mut("test").unwrap().command =
                "/nonexistent/something-bg-test".into();
            assert!(harness.runner.run_by_key("test").is_err());
            let failure = harness.result().failure.unwrap();
            assert!(failure.summary.contains("Could not start command"));
            assert!(harness.history().contains(&failure.failure_time_label()));
            assert!(harness.events.lock().unwrap().try_recv().is_err());
        }
    }

    #[test]
    fn silent_success_is_logged_only_after_exit_without_a_notification() {
        let harness = Harness::new("printf 'finished'", "silent");
        harness.runner.run_by_key("test").unwrap();
        let history = harness.history();
        assert!(history.contains("[OK]"));
        assert!(history.contains("Exit code: 0"));
        assert!(history.contains("finished"));
        assert!(harness.events.lock().unwrap().try_recv().is_err());
    }

    #[test]
    fn notify_success_keeps_completion_output() {
        let harness = Harness::new("printf 'finished'", "notify");
        harness.runner.run_by_key("test").unwrap();
        let result = harness.result();
        assert!(result.success);
        assert!(result.failure.is_none());
        assert_eq!(result.output, "finished");
    }

    #[test]
    fn signal_termination_reports_signal_instead_of_a_fake_exit_code() {
        let harness = Harness::new("kill -TERM $$", "silent");
        harness.runner.run_by_key("test").unwrap();
        let failure = harness.result().failure.unwrap();
        assert!(failure.summary.contains("signal"));
        assert!(failure.summary.contains("15"));
        assert!(!failure.summary.contains("-1"));
    }

    #[test]
    fn large_failed_output_is_bounded_without_blocking_completion() {
        let harness = Harness::new(
            "head -c 200000 /dev/zero; head -c 200000 /dev/zero >&2; echo last-error >&2; exit 1",
            "silent",
        );
        harness.runner.run_by_key("test").unwrap();
        let failure = harness.result().failure.unwrap();
        assert!(failure.stdout.starts_with("[Earlier output truncated]"));
        assert!(failure.stderr.starts_with("[Earlier output truncated]"));
        assert!(failure.stderr.ends_with("last-error\n"));
    }
}
