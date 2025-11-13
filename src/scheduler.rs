// src/scheduler.rs
//
// Cron-based task scheduler for Something in the Background.
// Handles scheduling and execution of periodic tasks based on cron expressions.

use chrono::{DateTime, Local, Utc};
use croner::Cron;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::ScheduledTaskConfig;

/// Represents a scheduled task with its configuration and runtime state
#[derive(Clone, Debug)]
pub struct ScheduledTask {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cron_schedule: String,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    cron: Option<Cron>,
}

impl ScheduledTask {
    /// Create a new ScheduledTask from configuration
    pub fn new(config: &ScheduledTaskConfig) -> Result<Self, String> {
        let cron = Cron::from_str(&config.cron_schedule).map_err(|e| {
            format!(
                "Failed to parse cron schedule '{}': {}",
                config.cron_schedule, e
            )
        })?;

        let now = Utc::now();
        let next_run = cron.find_next_occurrence(&now, false).ok();

        Ok(Self {
            name: config.name.clone(),
            command: config.command.clone(),
            args: config.args.clone(),
            cron_schedule: config.cron_schedule.clone(),
            last_run: None,
            next_run,
            cron: Some(cron),
        })
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
    pub fn should_run(&self, now: &DateTime<Utc>) -> bool {
        if let Some(next_run) = &self.next_run {
            now >= next_run
        } else {
            false
        }
    }

    /// Update next run time after execution
    pub fn update_next_run(&mut self) {
        if let Some(ref cron) = self.cron {
            let now = Utc::now();
            self.last_run = Some(now);
            self.next_run = cron.find_next_occurrence(&now, false).ok();
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
}

impl TaskScheduler {
    /// Create a new TaskScheduler
    pub fn new(path: String) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            path,
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Add a scheduled task
    pub fn add_task(&self, key: String, config: &ScheduledTaskConfig) -> Result<(), String> {
        let task = ScheduledTask::new(config)?;
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(key, task);
        Ok(())
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

        thread::spawn(move || {
            info!("Task scheduler started");

            while *running.lock().unwrap() {
                let now = Utc::now();
                let mut tasks_guard = tasks.lock().unwrap();

                for (key, task) in tasks_guard.iter_mut() {
                    if task.should_run(&now) {
                        debug!("Task '{}' is due to run", key);
                        if let Err(e) = task.execute(&path) {
                            error!("Task '{}' execution failed: {}", key, e);
                        }
                    }
                }

                drop(tasks_guard);

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
        if let Some(task) = tasks.get_mut(key) {
            task.execute(&self.path)
        } else {
            Err(format!("Task '{}' not found", key))
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
            // This is a basic implementation - you may want to enhance this
            if cron_pattern == "0 * * * *" {
                return "Every hour".to_string();
            }
            if cron_pattern == "0 0 * * *" {
                return "Every day at midnight".to_string();
            }
            if cron_pattern.starts_with("0 ") && cron_pattern.matches(' ').count() == 4 {
                // Pattern like "0 6 * * *" (at specific hour)
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

/// Format a DateTime for display in local timezone
pub fn format_last_run(last_run: &Option<DateTime<Utc>>) -> String {
    match last_run {
        Some(dt) => {
            // Convert UTC to local timezone
            let local_time = dt.with_timezone(&Local);
            // Format as a readable date/time string in 24-hour format
            // Example: "Jan 13, 2025 at 15:30"
            local_time.format("%b %d, %Y at %H:%M").to_string()
        }
        None => "Never".to_string(),
    }
}
