use std::sync::{Arc, Mutex};

use log::{error, info, warn};
use something_bg_core::config::Config;
use something_bg_core::scheduler::TaskScheduler;
use something_bg_core::tunnel::TunnelManager;

use crate::paths::WindowsPaths;

/// Shared application state for the Windows shell.
pub struct AppState {
    pub tunnel_manager: TunnelManager,
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
}
