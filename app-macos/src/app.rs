// src/app.rs
//
// Defines the `App` structure holding shared state (commands, active tunnels).
// Also provides methods for cleanup or other global operations.

use log::{error, info, warn};
use objc2::rc::Retained;
use objc2_app_kit::NSStatusItem;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use something_bg_core::config::Config;
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
    pub task_scheduler: TaskScheduler,
    pub paths: Arc<MacPaths>,
    pub status_item: Option<Arc<Mutex<StatusItemWrapper>>>,
}

impl App {
    /// Creates a new `App` with commands loaded from config file.
    pub fn new() -> Self {
        let paths = Arc::new(MacPaths::default());

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
            active_tunnels: Arc::new(Mutex::new(HashSet::new())),
            env_path: config.get_path(),
        };

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

        Self {
            tunnel_manager,
            task_scheduler,
            paths,
            status_item: None,
        }
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
}
