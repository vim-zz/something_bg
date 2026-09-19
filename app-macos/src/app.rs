// src/app.rs
//
// Defines the `App` structure holding shared state (commands, active tunnels).
// Also provides methods for cleanup or other global operations.

use log::{error, info, warn};
use objc2::rc::Retained;
use objc2_app_kit::NSStatusItem;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use something_bg_core::command::{CommandRunner, format_duration as format_elapsed};
use something_bg_core::config::{Config, ConfigMonitor, LoginSettingUpdate};
use something_bg_core::platform::AppPaths;
use something_bg_core::scheduler::TaskScheduler;
use something_bg_core::tunnel::TunnelManager;

use crate::paths::MacPaths;

// Wrapper type to make the status item thread-safe
pub struct StatusItemWrapper(pub Retained<NSStatusItem>);
unsafe impl Send for StatusItemWrapper {}
unsafe impl Sync for StatusItemWrapper {}

/// Primary application structure. Contains references to any data that
/// must be shared across modules (e.g., commands, active tunnels).
pub struct App {
    pub tunnel_manager: TunnelManager,
    pub tunnel_names: Mutex<HashMap<String, String>>,
    pub command_runner: Mutex<CommandRunner>,
    pub task_scheduler: TaskScheduler,
    pub paths: Arc<MacPaths>,
    pub status_item: Option<Arc<Mutex<StatusItemWrapper>>>,
    config_monitor: ConfigMonitor,
    start_at_login: Mutex<Option<bool>>,
}

impl App {
    /// Creates a new `App` with commands loaded from config file.
    pub fn new() -> (Self, Config) {
        let paths = Arc::new(MacPaths::default());

        // Load configuration from TOML file
        let (config, config_contents) = match Config::load_with_snapshot(paths.as_ref()) {
            Ok((config, contents)) => {
                info!("Loaded configuration successfully");
                (config, Some(contents))
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                warn!("Using default configuration");
                (Config::default(), None)
            }
        };

        let commands = config.to_tunnel_commands();
        let path = config.get_path();

        // Initialize the tunnel manager
        let tunnel_manager = TunnelManager::new(commands, config.get_path());

        // Initialize the command runner
        let mut command_runner = CommandRunner::new(config.get_path());
        let history_log = paths
            .config_path()
            .parent()
            .unwrap()
            .join("command_history.log");
        command_runner.set_history_path(history_log);

        // Set macOS notify callback using native UserNotifications
        // (shows the app icon instead of Script Editor)
        command_runner.set_notify_callback(std::sync::Arc::new(|event| {
            if event.is_running {
                send_notification(event.name, "\u{23f3} Running...");
                return;
            }
            let time_str = match event.elapsed {
                Some(d) if d.as_secs() >= 2 => format!(" ({})", format_elapsed(d)),
                _ => String::new(),
            };
            let status = if event.success {
                "\u{2705} Completed"
            } else {
                "\u{274c} Failed"
            };
            let body = if event.output.is_empty() {
                format!("{}{}", status, time_str)
            } else {
                format!("{}{}\n{}", status, time_str, event.output)
            };
            send_notification(event.name, &body);
        }));

        // Set macOS terminal callback using osascript
        command_runner.set_terminal_callback(std::sync::Arc::new(|command, args| {
            let full_cmd = if args.is_empty() {
                command.to_string()
            } else {
                format!(
                    "{} {}",
                    command,
                    args.iter()
                        .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            let script = format!(
                "tell application \"Terminal\" to do script \"{}\"",
                full_cmd.replace('\\', "\\\\").replace('"', "\\\"")
            );
            if let Err(e) = std::process::Command::new("osascript")
                .args(["-e", &script])
                .spawn()
            {
                log::warn!("Failed to open Terminal: {}", e);
            }
        }));

        // Register commands from config
        command_runner.register_all(&config.commands);

        // Initialize the task scheduler
        let task_scheduler = TaskScheduler::new(path, paths.as_ref());

        // Add scheduled tasks from config
        for (key, task_config) in &config.schedules {
            if let Err(e) = task_scheduler.add_task(key.clone(), task_config) {
                error!("Failed to add scheduled task '{}': {}", key, e);
            }
        }

        // Save initial states (including calculated next_run values) to disk
        task_scheduler.save_states();
        info!("Saved initial task states to disk");

        // Start the scheduler
        task_scheduler.start();
        info!(
            "Task scheduler started with {} tasks",
            config.schedules.len()
        );

        // Check for missed tasks on startup (before returning Self)
        info!("Checking for missed tasks on app startup...");
        task_scheduler.check_and_run_missed_tasks();

        let app = Self {
            tunnel_manager,
            tunnel_names: Mutex::new(
                config
                    .tunnels
                    .iter()
                    .map(|(key, tunnel)| (key.clone(), tunnel.name.clone()))
                    .collect(),
            ),
            command_runner: Mutex::new(command_runner),
            task_scheduler,
            paths: paths.clone(),
            status_item: None,
            start_at_login: Mutex::new(
                config_contents
                    .as_ref()
                    .map(|_| config.settings.start_at_login),
            ),
            config_monitor: ConfigMonitor::new(paths.config_path(), config_contents),
        };

        (app, config)
    }

    pub fn set_status_item(&mut self, item: Retained<NSStatusItem>) {
        self.status_item = Some(Arc::new(Mutex::new(StatusItemWrapper(item))));
    }

    pub fn get_status_item(&self) -> Option<Retained<NSStatusItem>> {
        self.status_item
            .as_ref()
            .and_then(|wrapper| wrapper.lock().ok().map(|guard| guard.0.clone()))
    }

    /// Cleans up any active tunnels and stops the scheduler. Called on app termination.
    pub fn cleanup_tunnels(&self) {
        self.tunnel_manager.cleanup();
        self.task_scheduler.stop();
    }

    /// Called when the system wakes from sleep to check for and run any missed scheduled tasks
    pub fn handle_wake_from_sleep(&self) {
        info!("System woke from sleep - restarting active tunnels and checking tasks");

        // Recycle any tunnels that were active before sleep to ensure fresh connections.
        self.tunnel_manager.restart_active_tunnels();

        // Resume scheduled task handling after wake.
        self.task_scheduler.check_and_run_missed_tasks();
    }

    pub fn config_path(&self) -> std::path::PathBuf {
        self.paths.config_path()
    }

    pub fn config_changed(&self) -> bool {
        self.config_monitor.has_changed().unwrap_or_else(|e| {
            warn!("Failed to check config for changes: {e}");
            false
        })
    }

    pub fn apply_login_preference(&self) -> Result<(), String> {
        // A malformed config must not change the user's existing OS preference.
        if let Some(enabled) = *self.start_at_login.lock().unwrap() {
            let status = crate::login_item::apply_preference(enabled)?;
            if status == crate::login_item::Status::RequiresApproval {
                send_login_notification(
                    "Allow Something in the Background in System Settings > General > Login Items. You can open it from the app’s Settings menu.",
                );
            }
        }
        Ok(())
    }

    pub fn toggle_start_at_login(&self) -> Result<(), String> {
        let previous = crate::login_item::status()?;
        let enabled = !previous.requested();
        let update = LoginSettingUpdate::prepare(self.paths.as_ref(), enabled)
            .map_err(|error| error.to_string())?;
        crate::login_item::set_enabled(enabled)?;
        if let Err(error) = update.save() {
            // Restore the OS preference if the config could not be saved.
            return match crate::login_item::set_enabled(previous.requested()) {
                Ok(_) => Err(format!("Could not save Start at Login: {error}")),
                Err(rollback) => Err(format!(
                    "Could not save Start at Login: {error}. Restoring the previous login setting also failed: {rollback}"
                )),
            };
        }
        *self.start_at_login.lock().unwrap() = Some(enabled);
        self.config_monitor
            .mark_setting_applied(&update.original, update.updated);
        Ok(())
    }

    pub fn reload_config(&self) -> Result<Config, String> {
        let (config, contents) =
            Config::load_with_snapshot(self.paths.as_ref()).map_err(|e| e.to_string())?;
        let path = config.get_path();

        // Fail before replacing runtime commands/menu data if macOS rejects the preference.
        crate::login_item::apply_preference(config.settings.start_at_login)?;
        self.task_scheduler
            .reconfigure(path.clone(), &config.schedules)?;
        self.tunnel_manager
            .reconfigure(config.to_tunnel_commands(), path.clone());
        self.command_runner
            .lock()
            .unwrap()
            .reconfigure(path, &config.commands);
        *self.tunnel_names.lock().unwrap() = config
            .tunnels
            .iter()
            .map(|(key, tunnel)| (key.clone(), tunnel.name.clone()))
            .collect();
        *self.start_at_login.lock().unwrap() = Some(config.settings.start_at_login);
        self.config_monitor.mark_applied(contents);

        info!("Reloaded configuration successfully");
        Ok(config)
    }
}

pub use crate::notifications::{
    send_login_notification, send_notification, send_tunnel_failure, setup_notification_center,
};
