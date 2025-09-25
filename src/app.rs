// src/app.rs
//
// Defines the `App` structure holding shared state (commands, active tunnels).
// Also provides methods for cleanup or other global operations.

use cocoa::base::id;
use log::{error, info, warn};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::tunnel::TunnelManager;

// Wrapper type to make the status item thread-safe
pub struct StatusItemWrapper(pub id);
unsafe impl Send for StatusItemWrapper {}
unsafe impl Sync for StatusItemWrapper {}

/// Primary application structure. Contains references to any data that
/// must be shared across modules (e.g., commands, active tunnels).
pub struct App {
    pub tunnel_manager: TunnelManager,
    pub status_item: Option<Arc<Mutex<StatusItemWrapper>>>,
}

impl App {
    /// Creates a new `App` with commands loaded from config file.
    pub fn new() -> Self {
        // Load configuration from TOML file
        let commands = match Config::load() {
            Ok(config) => {
                info!("Loaded configuration successfully");
                config.to_tunnel_commands()
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                warn!("Using default configuration");
                Config::default().to_tunnel_commands()
            }
        };

        // Initialize the tunnel manager
        let tunnel_manager = TunnelManager {
            commands_config: Arc::new(Mutex::new(commands)),
            active_tunnels: Arc::new(Mutex::new(HashSet::new())),
        };

        Self {
            tunnel_manager,
            status_item: None,
        }
    }

    pub fn set_status_item(&mut self, item: id) {
        self.status_item = Some(Arc::new(Mutex::new(StatusItemWrapper(item))));
    }

    pub fn get_status_item(&self) -> Option<id> {
        self.status_item
            .as_ref()
            .and_then(|wrapper| wrapper.lock().ok().map(|guard| guard.0))
    }

    /// Cleans up any active tunnels. Called on app termination.
    pub fn cleanup_tunnels(&self) {
        self.tunnel_manager.cleanup();
    }
}
