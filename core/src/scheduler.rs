// src/scheduler.rs
//
// Cron-based task scheduler for Something in the Background.
// Handles scheduling and execution of periodic tasks based on cron expressions.

use chrono::{DateTime, Datelike, Local};
use croner::Cron;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::ScheduledTaskConfig;
use crate::platform::AppPaths;

/// Structure for persisting scheduled task state
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TaskState {
    last_run: Option<DateTime<Local>>,
    next_run: Option<DateTime<Local>>,
}

/// Load persisted task states from disk
fn load_task_states(path: &PathBuf) -> HashMap<String, TaskState> {
    if !path.exists() {
        return HashMap::new();
    }

    match fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(states) => {
                info!("Loaded task states from {}", path.display());
                states
            }
            Err(e) => {
                warn!("Failed to parse task state file: {}", e);
                HashMap::new()
            }
        },
        Err(e) => {
            warn!("Failed to read task state file: {}", e);
            HashMap::new()
        }
    }
}

/// Save task states to disk
fn save_task_states(path: &PathBuf, states: &HashMap<String, TaskState>) {
    // Ensure the directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            error!("Failed to create state directory: {}", e);
            return;
        }
    }

    match toml::to_string_pretty(&states) {
        Ok(toml_content) => {
            if let Err(e) = fs::write(&path, toml_content) {
                error!("Failed to write task state file: {}", e);
            } else {
                debug!("Saved task states to {}", path.display());
            }
        }
        Err(e) => {
            error!("Failed to serialize task states: {}", e);
        }
    }
}

/// Represents a scheduled task with its configuration and runtime state
#[derive(Clone, Debug)]
pub struct ScheduledTask {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cron_schedule: String,
    pub last_run: Option<DateTime<Local>>,
    pub next_run: Option<DateTime<Local>>,
    cron: Option<Cron>,
}

impl ScheduledTask {
    /// Create a new ScheduledTask from configuration and persisted state
    fn new(config: &ScheduledTaskConfig, state: Option<&TaskState>) -> Result<Self, String> {
        info!(
            "Creating task '{}' with schedule '{}'",
            config.name, config.cron_schedule
        );

        let cron = Cron::from_str(&config.cron_schedule).map_err(|e| {
            format!(
                "Failed to parse cron schedule '{}': {}",
                config.cron_schedule, e
            )
        })?;

        let now = Local::now();

        // Load or calculate next_run
        let next_run = if let Some(state) = state {
            if let Some(saved_next_run) = state.next_run {
                info!(
                    "Task '{}': loaded next_run from file: {}",
                    config.name, saved_next_run
                );
                Some(saved_next_run)
            } else {
                // State exists but no next_run - calculate it
                info!(
                    "Task '{}': no saved next_run, calculating from now",
                    config.name
                );
                Self::calculate_next_run(&cron, &now, &config.name)
            }
        } else {
            // No state at all - first time
            info!(
                "Task '{}': first time, calculating next_run from now",
                config.name
            );
            Self::calculate_next_run(&cron, &now, &config.name)
        };

        let last_run = state.and_then(|s| s.last_run);

        info!(
            "Task '{}': initialized with last_run={:?}, next_run={:?}",
            config.name, last_run, next_run
        );

        Ok(Self {
            name: config.name.clone(),
            command: config.command.clone(),
            args: config.args.clone(),
            cron_schedule: config.cron_schedule.clone(),
            last_run,
            next_run,
            cron: Some(cron),
        })
    }

    /// Calculate next occurrence from a given time
    fn calculate_next_run(
        cron: &Cron,
        from_time: &DateTime<Local>,
        task_name: &str,
    ) -> Option<DateTime<Local>> {
        match cron.find_next_occurrence(from_time, false) {
            Ok(next) => {
                info!("Task '{}': calculated next_run = {}", task_name, next);
                Some(next)
            }
            Err(e) => {
                error!("Task '{}': failed to calculate next_run: {}", task_name, e);
                None
            }
        }
    }

    /// Get a human-readable description of the cron schedule
    pub fn get_schedule_description(&self) -> String {
        if let Some(ref cron) = self.cron {
            cron.pattern.to_string()
        } else {
            self.cron_schedule.clone()
        }
    }

    /// Check if the task should run now
    pub fn should_run(&self, now: &DateTime<Local>) -> bool {
        if let Some(next_run) = &self.next_run {
            now >= next_run
        } else {
            false
        }
    }

    /// Update next run time after execution
    pub fn update_next_run(&mut self) {
        if let Some(ref cron) = self.cron {
            let now = Local::now();
            self.last_run = Some(now);
            match cron.find_next_occurrence(&now, false) {
                Ok(next) => {
                    self.next_run = Some(next);
                    debug!(
                        "Task '{}': updated next_run to {} after execution at {}",
                        self.name, next, now
                    );
                }
                Err(e) => {
                    error!(
                        "Task '{}': failed to calculate next_run after execution at {}: {}",
                        self.name, now, e
                    );
                    self.next_run = None;
                }
            }
        }
    }

    /// Execute the scheduled task
    pub fn execute(&mut self, path: &str) -> Result<(), String> {
        info!(
            "Executing scheduled task '{}': {} {:?}",
            self.name, self.command, self.args
        );

        let result = Command::new(&self.command)
            .args(&self.args)
            .env("PATH", path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match result {
            Ok(_) => {
                self.update_next_run();
                info!(
                    "Successfully executed task '{}'. Next run: {:?}",
                    self.name, self.next_run
                );
                Ok(())
            }
            Err(e) => {
                let err_msg = format!("Failed to execute task '{}': {}", self.name, e);
                error!("{}", err_msg);
                Err(err_msg)
            }
        }
    }
}

/// Manages all scheduled tasks and handles their execution
pub struct TaskScheduler {
    tasks: Arc<Mutex<HashMap<String, ScheduledTask>>>,
    path: String,
    running: Arc<Mutex<bool>>,
    states: Arc<Mutex<HashMap<String, TaskState>>>,
    state_file: PathBuf,
}

impl TaskScheduler {
    /// Create a new TaskScheduler
    pub fn new<P: AppPaths>(path: String, paths: &P) -> Self {
        let state_file = paths.state_path();
        let states = load_task_states(&state_file);
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            path,
            running: Arc::new(Mutex::new(false)),
            states: Arc::new(Mutex::new(states)),
            state_file,
        }
    }

    /// Add a scheduled task
    pub fn add_task(&self, key: String, config: &ScheduledTaskConfig) -> Result<(), String> {
        // Check if we have a persisted state for this task
        let states = self.states.lock().unwrap();
        let state = states.get(&key);

        let task = ScheduledTask::new(config, state)?;
        drop(states);

        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(key, task);
        Ok(())
    }

    /// Save the current task states to disk
    pub fn save_states(&self) {
        let tasks = self.tasks.lock().unwrap();
        let mut states_map = HashMap::new();

        for (key, task) in tasks.iter() {
            states_map.insert(
                key.clone(),
                TaskState {
                    last_run: task.last_run,
                    next_run: task.next_run,
                },
            );
        }

        drop(tasks);

        // Update the states in memory
        let mut states = self.states.lock().unwrap();
        *states = states_map.clone();
        drop(states);

        // Save to disk
        save_task_states(&self.state_file, &states_map);
    }

    /// Get a copy of a specific task's state
    pub fn get_task(&self, key: &str) -> Option<ScheduledTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(key).cloned()
    }

    /// Get all tasks
    pub fn get_all_tasks(&self) -> HashMap<String, ScheduledTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.clone()
    }

    /// Start the scheduler background thread
    pub fn start(&self) {
        let mut running = self.running.lock().unwrap();
        if *running {
            warn!("Scheduler is already running");
            return;
        }
        *running = true;
        drop(running);

        let tasks = Arc::clone(&self.tasks);
        let path = self.path.clone();
        let running = Arc::clone(&self.running);
        let states = Arc::clone(&self.states);
        let state_file = self.state_file.clone();

        thread::spawn(move || {
            info!("Task scheduler started");

            while *running.lock().unwrap() {
                let now = Local::now();
                let mut tasks_guard = tasks.lock().unwrap();
                let mut states_changed = false;

                for (key, task) in tasks_guard.iter_mut() {
                    if task.should_run(&now) {
                        debug!("Task '{}' is due to run", key);
                        if let Err(e) = task.execute(&path) {
                            error!("Task '{}' execution failed: {}", key, e);
                        } else {
                            states_changed = true;
                        }
                    }
                }

                drop(tasks_guard);

                // Save states if any task was executed
                if states_changed {
                    let tasks = tasks.lock().unwrap();
                    let mut states_map = HashMap::new();

                    for (key, task) in tasks.iter() {
                        states_map.insert(
                            key.clone(),
                            TaskState {
                                last_run: task.last_run,
                                next_run: task.next_run,
                            },
                        );
                    }

                    drop(tasks);

                    let mut states_guard = states.lock().unwrap();
                    *states_guard = states_map.clone();
                    drop(states_guard);

                    save_task_states(&state_file, &states_map);
                }

                // Check every 30 seconds
                thread::sleep(Duration::from_secs(30));
            }

            info!("Task scheduler stopped");
        });
    }

    /// Stop the scheduler
    pub fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
        info!("Stopping task scheduler");
    }

    /// Manually trigger a task to run now
    pub fn run_task_now(&self, key: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().unwrap();
        let result = if let Some(task) = tasks.get_mut(key) {
            task.execute(&self.path)
        } else {
            Err(format!("Task '{}' not found", key))
        };

        drop(tasks);

        // Save states after manual execution
        if result.is_ok() {
            self.save_states();
        }

        result
    }

    /// Check for and run any missed scheduled tasks
    /// This is useful after the system wakes from sleep
    pub fn check_and_run_missed_tasks(&self) {
        let now = Local::now();
        let mut tasks = self.tasks.lock().unwrap();
        let mut any_task_run = false;

        info!(
            "Checking for missed scheduled tasks (current time: {})",
            now
        );

        for (key, task) in tasks.iter_mut() {
            info!(
                "Task '{}': schedule={}, next_run={:?}, last_run={:?}",
                key, task.cron_schedule, task.next_run, task.last_run
            );

            // A task is considered "missed" if:
            // 1. It has a next_run time scheduled
            // 2. That next_run time is in the past (we're past when it should have run)
            // 3. Either it has never run, or the last run was before the scheduled next_run
            if let Some(next_run) = task.next_run {
                let is_overdue = now >= next_run;
                let was_not_run_yet = task.last_run.is_none() || task.last_run.unwrap() < next_run;

                debug!(
                    "Task '{}': is_overdue={}, was_not_run_yet={}, would_run={}",
                    key,
                    is_overdue,
                    was_not_run_yet,
                    is_overdue && was_not_run_yet
                );

                if is_overdue && was_not_run_yet {
                    info!(
                        "Task '{}' was scheduled to run at {} but was missed. Running now.",
                        key, next_run
                    );

                    if let Err(e) = task.execute(&self.path) {
                        error!("Failed to run missed task '{}': {}", key, e);
                    } else {
                        any_task_run = true;
                    }
                }
            } else {
                info!("Task '{}' has no next_run scheduled", key);
            }
        }

        drop(tasks);

        // Save states if any task was run
        if any_task_run {
            self.save_states();
        }
    }
}

impl Drop for TaskScheduler {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Convert a cron pattern to a human-readable description
pub fn cron_to_human_readable(cron_pattern: &str) -> String {
    match Cron::from_str(cron_pattern) {
        Ok(cron) => {
            // Try to create a more user-friendly description
            let pattern = &cron.pattern;

            // Simple pattern matching for common cases
            if cron_pattern == "0 * * * *" {
                return "Every hour".to_string();
            }
            if cron_pattern == "0 0 * * *" {
                return "Every day at midnight".to_string();
            }
            if cron_pattern.starts_with("0 ") && cron_pattern.matches(' ').count() == 4 {
                // Pattern like "0 10 * * *" (at specific hour in local time)
                let parts: Vec<&str> = cron_pattern.split_whitespace().collect();
                if parts.len() == 5
                    && parts[1].parse::<u32>().is_ok()
                    && parts[2] == "*"
                    && parts[3] == "*"
                    && parts[4] == "*"
                {
                    let hour = parts[1].parse::<u32>().unwrap();
                    return format!("Every day at {}:00", hour);
                }
            }

            // Fall back to the pattern string
            pattern.to_string()
        }
        Err(_) => cron_pattern.to_string(),
    }
}

/// Format a DateTime for display
pub fn format_last_run(last_run: &Option<DateTime<Local>>) -> String {
    match last_run {
        Some(dt) => format_relative_datetime(dt),
        None => "Never".to_string(),
    }
}

/// Human-friendly relative datetime like "tomorrow at 10:00" or
/// "in 3 weeks (on Dec 21st, 2025 at 10:00)".
fn format_relative_datetime(dt: &DateTime<Local>) -> String {
    let now = Local::now();
    let date_diff = dt.date_naive().signed_duration_since(now.date_naive());
    let diff_days = date_diff.num_days();
    let time_part = dt.format("%H:%M").to_string();

    match diff_days {
        0 => format!("today at {time_part}"),
        1 => format!("tomorrow at {time_part}"),
        -1 => format!("yesterday at {time_part}"),
        2..=6 => format!("on {} at {time_part}", dt.format("%A")),
        -6..=-2 => format!("last {} at {time_part}", dt.format("%A")),
        _ => {
            let date_str = if dt.year() == now.year() {
                format!("{} {}", dt.format("%b"), ordinal(dt.day()))
            } else {
                format!("{} {}, {}", dt.format("%b"), ordinal(dt.day()), dt.year())
            };

            let relative = humantime_fmt::format_relative((*dt).into());
            format!("{relative} (on {date_str} at {time_part})")
        }
    }
}

/// Return ordinal suffix for a day (1st, 2nd, 3rd, 4th, ...).
fn ordinal(day: u32) -> String {
    let suffix = match day % 100 {
        11 | 12 | 13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };

    format!("{day}{suffix}")
}
