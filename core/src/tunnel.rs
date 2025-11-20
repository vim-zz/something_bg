//! Tunnel lifecycle management (platform-agnostic).
//! Handles starting/stopping configured commands and tracking active tunnels.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use log::{debug, error, info, warn};

use crate::config::Config;

#[derive(Clone)]
pub struct TunnelCommand {
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
}

/// Manages the lifecycle of tunnels (start, stop, cleanup).
/// Replaces the global static variables with owned fields.
pub struct TunnelManager {
    pub commands_config: Arc<Mutex<HashMap<String, TunnelCommand>>>,
    pub active_tunnels: Arc<Mutex<HashSet<String>>>,
}

impl TunnelManager {
    /// Toggles a tunnel by name (command_key) on or off.
    /// If turning on, spawns a thread to run the SSH command.
    /// If turning off, kills the process.
    /// Toggle a tunnel on/off. Returns `true` if any tunnels are active after the toggle.
    pub fn toggle(&self, command_key: &str, enable: bool) -> bool {
        if enable {
            // Mark tunnel active
            {
                let mut tunnels = self.active_tunnels.lock().unwrap();
                tunnels.insert(command_key.to_owned());
            }

            // Spawn thread
            let commands_config = self.commands_config.clone();
            let active_tunnels = self.active_tunnels.clone();
            let command_key = command_key.to_owned();

            thread::spawn(move || {
                let mut attempts = 0;

                // Define closure to check if tunnel is still active
                let is_active = || {
                    let tunnels = active_tunnels.lock().unwrap();
                    tunnels.contains(&command_key)
                };

                while is_active() && attempts < 5 {
                    let command = {
                        let cfg = commands_config.lock().unwrap();
                        cfg.get(&command_key).unwrap().clone()
                    };

                    info!(
                        "Spawning command: {} {:?} (attempt {})",
                        command.command, command.args, attempts
                    );

                    let mut cmd = Command::new(&command.command);

                    // Get PATH from config
                    let config_path = match Config::load() {
                        Ok(config) => config.get_path(),
                        Err(_) => "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/opt/homebrew/bin"
                            .to_string(),
                    };

                    // Update PATH to include configured paths
                    let new_path = cmd
                        .get_envs()
                        .find(|(key, _)| key == &OsStr::new("PATH"))
                        .map(|(_, value)| {
                            value.map(|path| format!("{}:{}", config_path, path.to_string_lossy()))
                        })
                        .flatten()
                        .unwrap_or_else(|| config_path.clone());

                    debug!("Update PATH to: {new_path}");
                    cmd.env("PATH", new_path);

                    match cmd
                        .args(&command.args)
                        // Discard the output (silence the process)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        Ok(mut child) => {
                            info!("Tunnel process started");
                            let _ = child.wait();
                        }
                        Err(e) => error!("Failed to start tunnel command: {}", e),
                    }

                    attempts += 1;
                }

                if attempts == 5 {
                    warn!("Failed to start command after 5 attempts");
                }
            });
        } else {
            // Remove from active set
            {
                let mut tunnels = self.active_tunnels.lock().unwrap();
                tunnels.remove(command_key);
            }

            // Kill the process
            let cmd_data = {
                let cfg = self.commands_config.lock().unwrap();
                cfg.get(command_key).unwrap().clone()
            };

            info!("Stopping command: {} {:?}", cmd_data.command, cmd_data.args);
            match Command::new(&cmd_data.kill_command)
                .args(&cmd_data.kill_args)
                .output()
            {
                Ok(_) => debug!("Tunnel stopped successfully"),
                Err(e) => error!("Failed to stop tunnel process: {}", e),
            }
        }

        self.has_active_tunnels()
    }

    /// Cleans up all tunnels when the app terminates.
    pub fn cleanup(&self) {
        let config = self.commands_config.lock().unwrap();
        let mut active = self.active_tunnels.lock().unwrap();

        for key in active.iter() {
            debug!("Cleaning up tunnel: {}", key);
            if let Some(cmd_data) = config.get(key) {
                match Command::new(&cmd_data.kill_command)
                    .args(&cmd_data.kill_args)
                    .output()
                {
                    Ok(_) => debug!("Process stopped for {}", key),
                    Err(e) => error!("Failed to stop process for {}: {}", key, e),
                }
            }
        }

        // Clear all active
        active.clear();
        debug!("All tunnels cleaned up");
    }
}

impl TunnelManager {
    pub fn has_active_tunnels(&self) -> bool {
        let tunnels = self.active_tunnels.lock().unwrap();
        !tunnels.is_empty()
    }
}
