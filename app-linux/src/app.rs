use std::sync::{Arc, Mutex};

use log::{error, info, warn};
use something_bg_core::command::CommandRunner;
use something_bg_core::config::Config;
use something_bg_core::scheduler::TaskScheduler;
use something_bg_core::tunnel::TunnelManager;

use crate::paths::LinuxPaths;

/// Shared application state for the Linux shell.
/// Holds the tunnel manager and scheduler so menu handlers can drive them.
pub struct AppState {
    pub tunnel_manager: TunnelManager,
    pub command_runner: CommandRunner,
    pub scheduler: Arc<TaskScheduler>,
    pub paths: Arc<LinuxPaths>,
}

impl AppState {
    pub fn new() -> (Self, Config) {
        let paths = Arc::new(LinuxPaths::default());

        // Load configuration from TOML file
        let config = match Config::load_with(paths.as_ref()) {
            Ok(config) => {
                info!("Loaded configuration successfully");
                config
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                warn!("Using default configuration");
                Config::default()
            }
        };

        let commands = config.to_tunnel_commands();
        let path = config.get_path();

        // Initialize the tunnel manager
        let tunnel_manager = TunnelManager {
            commands_config: Arc::new(Mutex::new(commands)),
            active_tunnels: Arc::new(Mutex::new(Default::default())),
            env_path: config.get_path(),
        };

        // Initialize the command runner
        let mut command_runner = CommandRunner::new(config.get_path());
        let history_log = paths
            .config_path()
            .parent()
            .unwrap()
            .join("command_history.log");
        command_runner.set_history_path(history_log);

        // Set Linux notify callback using notify-send
        command_runner.set_notify_callback(std::sync::Arc::new(|event| {
            if event.is_running {
                if let Err(e) = std::process::Command::new("notify-send")
                    .args([event.name, "\u{23f3} Running..."])
                    .spawn()
                {
                    log::warn!("Failed to send notification: {}", e);
                }
                return;
            }
            let title = if event.success {
                format!("{} completed", event.name)
            } else {
                format!("{} failed", event.name)
            };
            if let Err(e) = std::process::Command::new("notify-send")
                .args([&title, event.output])
                .spawn()
            {
                log::warn!("Failed to send notification: {}", e);
            }
        }));

        // Set Linux terminal callback
        command_runner.set_terminal_callback(std::sync::Arc::new(|command, args| {
            let full_cmd = if args.is_empty() {
                command.to_string()
            } else {
                format!("{} {}", command, args.join(" "))
            };
            if let Err(e) = std::process::Command::new("x-terminal-emulator")
                .args(["-e", &full_cmd])
                .spawn()
            {
                log::warn!("Failed to open terminal (trying xterm): {}", e);
                if let Err(e2) = std::process::Command::new("xterm")
                    .args(["-e", &full_cmd])
                    .spawn()
                {
                    log::error!("Failed to open xterm: {}", e2);
                }
            }
        }));

        // Register commands from config
        command_runner.register_all(&config.commands);

        // Initialize the task scheduler
        let scheduler = Arc::new(TaskScheduler::new(path, paths.as_ref()));

        // Add scheduled tasks from config
        for (key, task_config) in &config.schedules {
            if let Err(e) = scheduler.add_task(key.clone(), task_config) {
                error!("Failed to add scheduled task '{}': {}", key, e);
            }
        }

        // Save initial states (including calculated next_run values) to disk
        scheduler.save_states();
        info!("Saved initial task states to disk");

        // Start the scheduler
        scheduler.start();
        info!(
            "Task scheduler started with {} tasks",
            config.schedules.len()
        );

        // Check for missed tasks on startup (before returning Self)
        info!("Checking for missed tasks on app startup...");
        scheduler.check_and_run_missed_tasks();

        (
            Self {
                tunnel_manager,
                command_runner,
                scheduler,
                paths,
            },
            config,
        )
    }

    pub fn cleanup(&self) {
        self.tunnel_manager.cleanup();
        self.scheduler.stop();
    }

    /// Restart active tunnels and catch up on missed tasks after a detected wake.
    pub fn handle_wake(&self) {
        info!("Detected wake from sleep; restarting active tunnels and checking tasks");
        self.tunnel_manager.restart_active_tunnels();
        self.scheduler.check_and_run_missed_tasks();
    }
}
