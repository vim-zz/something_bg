use std::sync::{Arc, Mutex};

use log::{error, info, warn};
use something_bg_core::command::CommandRunner;
use something_bg_core::config::Config;
use something_bg_core::scheduler::TaskScheduler;
use something_bg_core::tunnel::TunnelManager;

use crate::paths::WindowsPaths;

/// Shared application state for the Windows shell.
pub struct AppState {
    pub tunnel_manager: TunnelManager,
    pub command_runner: CommandRunner,
    pub scheduler: Arc<TaskScheduler>,
    pub paths: Arc<WindowsPaths>,
}

impl AppState {
    pub fn new() -> (Self, Config) {
        let paths = Arc::new(WindowsPaths::default());

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

        // Set Windows notify callback using PowerShell toast notification
        command_runner.set_notify_callback(std::sync::Arc::new(|event| {
            if event.is_running {
                return; // Windows toast notifications auto-dismiss; skip running indicator
            }
            let title = if event.success {
                format!("{} completed", event.name)
            } else {
                format!("{} failed", event.name)
            };
            let body = event.output.replace('\'', "''");
            let ps_script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                 $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                 $text = $xml.GetElementsByTagName('text'); \
                 $text[0].AppendChild($xml.CreateTextNode('{}')) > $null; \
                 $text[1].AppendChild($xml.CreateTextNode('{}')) > $null; \
                 $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('something_bg').Show($toast)",
                title.replace('\'', "''"),
                body
            );
            if let Err(e) = std::process::Command::new("powershell")
                .args(["-Command", &ps_script])
                .spawn()
            {
                log::warn!("Failed to send notification: {}", e);
            }
        }));

        // Set Windows terminal callback
        command_runner.set_terminal_callback(std::sync::Arc::new(|command, args| {
            let full_cmd = if args.is_empty() {
                command.to_string()
            } else {
                format!("{} {}", command, args.join(" "))
            };
            if let Err(e) = std::process::Command::new("cmd")
                .args(["/C", "start", "cmd", "/K", &full_cmd])
                .spawn()
            {
                log::warn!("Failed to open terminal: {}", e);
            }
        }));

        // Register commands from config
        command_runner.register_all(&config.commands);

        let scheduler = Arc::new(TaskScheduler::new(path, paths.as_ref()));
        for (key, task_config) in &config.schedules {
            if let Err(e) = scheduler.add_task(key.clone(), task_config) {
                error!("Failed to add scheduled task '{}': {}", key, e);
            }
        }

        scheduler.save_states();
        info!("Saved initial task states to disk");
        scheduler.start();
        info!(
            "Task scheduler started with {} tasks",
            config.schedules.len()
        );
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
