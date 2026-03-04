//! One-time command runner with configurable output modes.
//!
//! Each command can run in one of three modes:
//! - **Silent**: fire-and-forget, no output captured
//! - **Notify**: run in background, capture output, send notification on completion
//! - **Terminal**: open a terminal emulator and execute the command there

use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use crate::config::CommandConfig;

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
}

/// Callback invoked for "notify" mode commands (progress and completion).
pub type NotifyCallback = Arc<dyn Fn(&NotifyEvent) + Send + Sync>;

/// Callback invoked for "terminal" mode.
/// Parameters: (command, args)
pub type TerminalCallback = Arc<dyn Fn(&str, &[String]) + Send + Sync>;

/// Runs one-time commands with configurable output handling.
pub struct CommandRunner {
    env_path: String,
    commands: HashMap<String, CommandEntry>,
    notify_cb: Option<NotifyCallback>,
    terminal_cb: Option<TerminalCallback>,
    history_path: Option<PathBuf>,
}

impl CommandRunner {
    pub fn new(env_path: String) -> Self {
        Self {
            env_path,
            commands: HashMap::new(),
            notify_cb: None,
            terminal_cb: None,
            history_path: None,
        }
    }

    pub fn set_history_path(&mut self, path: PathBuf) {
        self.history_path = Some(path);
    }

    /// Return the history log path, if configured.
    pub fn history_path(&self) -> Option<&std::path::Path> {
        self.history_path.as_deref()
    }

    pub fn set_notify_callback(&mut self, cb: NotifyCallback) {
        self.notify_cb = Some(cb);
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
            OutputMode::Silent => self.run_silent(entry),
            OutputMode::Notify => self.run_notify(entry),
            OutputMode::Terminal => self.run_terminal(entry),
        }
    }

    fn run_silent(&self, entry: &CommandEntry) -> Result<(), String> {
        let result = Command::new(&entry.command)
            .args(&entry.args)
            .env("PATH", &self.env_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match &result {
            Ok(_) => {
                append_history(
                    &self.history_path,
                    &entry.name,
                    true,
                    "(spawned, silent mode)",
                );
                debug!("Spawned silent command: {}", entry.name);
            }
            Err(e) => {
                let msg = format!("Failed to spawn: {}", e);
                append_history(&self.history_path, &entry.name, false, &msg);
            }
        }

        result
            .map(|_| ())
            .map_err(|e| format!("Failed to spawn '{}': {}", entry.command, e))
    }

    fn run_notify(&self, entry: &CommandEntry) -> Result<(), String> {
        let cb = self.notify_cb.clone();
        let name = entry.name.clone();
        let command = entry.command.clone();
        let args = entry.args.clone();
        let env_path = self.env_path.clone();
        let history_path = self.history_path.clone();

        thread::spawn(move || {
            // Send "running" notification only if the command takes > 2 seconds
            let cb_for_timer = cb.clone();
            let name_for_timer = name.clone();
            let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let done_clone = done.clone();
            thread::spawn(move || {
                thread::sleep(std::time::Duration::from_secs(2));
                if !done_clone.load(std::sync::atomic::Ordering::Relaxed)
                    && let Some(cb) = cb_for_timer
                {
                    cb(&NotifyEvent {
                        name: &name_for_timer,
                        success: true,
                        output: "",
                        elapsed: None,
                        is_running: true,
                    });
                }
            });

            let start_time = std::time::Instant::now();
            let result = Command::new(&command)
                .args(&args)
                .env("PATH", &env_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();

            let elapsed = start_time.elapsed();
            done.store(true, std::sync::atomic::Ordering::Relaxed);

            match result {
                Ok(output) => {
                    let success = output.status.success();
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = if stderr.is_empty() {
                        stdout.to_string()
                    } else if stdout.is_empty() {
                        stderr.to_string()
                    } else {
                        format!("{}\n{}", stdout, stderr)
                    };

                    // Log full output to history
                    let history_entry = format!(
                        "Exit code: {} (took {})\n{}",
                        output.status.code().unwrap_or(-1),
                        format_duration(elapsed),
                        combined
                    );
                    append_history(&history_path, &name, success, &history_entry);

                    // Take last 5 lines for notification
                    let all_lines: Vec<&str> = combined.lines().collect();
                    let start = all_lines.len().saturating_sub(5);
                    let last_lines = all_lines[start..].join("\n");

                    info!(
                        "Command '{}' finished (success={}, took {})",
                        name,
                        success,
                        format_duration(elapsed)
                    );

                    if let Some(cb) = cb {
                        cb(&NotifyEvent {
                            name: &name,
                            success,
                            output: &last_lines,
                            elapsed: Some(elapsed),
                            is_running: false,
                        });
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to execute: {}", e);
                    append_history(&history_path, &name, false, &msg);
                    error!("Failed to run command '{}': {}", name, e);
                    if let Some(cb) = cb {
                        cb(&NotifyEvent {
                            name: &name,
                            success: false,
                            output: &msg,
                            elapsed: None,
                            is_running: false,
                        });
                    }
                }
            }
        });

        Ok(())
    }

    fn run_terminal(&self, entry: &CommandEntry) -> Result<(), String> {
        if let Some(cb) = &self.terminal_cb {
            cb(&entry.command, &entry.args);
            append_history(
                &self.history_path,
                &entry.name,
                true,
                "(opened in terminal)",
            );
            Ok(())
        } else {
            Err("No terminal callback configured".to_string())
        }
    }
}

/// Append a timestamped entry to the command history log.
fn append_history(path: &Option<PathBuf>, name: &str, success: bool, output: &str) {
    let Some(path) = path else { return };

    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let status = if success { "OK" } else { "FAILED" };
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
