// src/app.rs
//
// Defines the `App` structure holding shared state (commands, active tunnels).
// Also provides methods for cleanup or other global operations.

use log::{error, info, warn};
use objc2::rc::Retained;
use objc2_app_kit::NSStatusItem;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::scheduler::TaskScheduler;
use crate::tunnel::TunnelManager;

// Wrapper type to make the status item thread-safe
pub struct StatusItemWrapper(pub Retained<NSStatusItem>);
unsafe impl Send for StatusItemWrapper {}
unsafe impl Sync for StatusItemWrapper {}

/// Primary application structure. Contains references to any data that
/// must be shared across modules (e.g., commands, active tunnels).
pub struct App {
    pub tunnel_manager: TunnelManager,
    pub task_scheduler: TaskScheduler,
    pub status_item: Option<Arc<Mutex<StatusItemWrapper>>>,
}

impl App {
    /// Creates a new `App` with commands loaded from config file.
    pub fn new() -> Self {
        // Load configuration from TOML file
        let config = match Config::load() {
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
        };

        // Initialize the task scheduler
        let task_scheduler = TaskScheduler::new(path);

        // Add scheduled tasks from config
        for (key, task_config) in &config.schedules {
            if let Err(e) = task_scheduler.add_task(key.clone(), task_config) {
                error!("Failed to add scheduled task '{}': {}", key, e);
            }
        }

        // Start the scheduler
        task_scheduler.start();
        info!(
            "Task scheduler started with {} tasks",
            config.schedules.len()
        );

        Self {
            tunnel_manager,
            task_scheduler,
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
}
