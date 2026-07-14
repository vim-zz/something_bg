//! Tunnel lifecycle management (platform-agnostic).
//! Handles starting/stopping configured commands and tracking active tunnels.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};

#[derive(Clone, PartialEq, Eq)]
pub struct TunnelCommand {
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
}

/// Manages the lifecycle of tunnels (start, stop, cleanup).
/// Replaces the global static variables with owned fields.
#[derive(Clone)]
pub struct TunnelManager {
    pub commands_config: Arc<Mutex<HashMap<String, TunnelCommand>>>,
    pub active_tunnels: Arc<Mutex<HashSet<String>>>,
    pub active_commands: Arc<Mutex<HashMap<String, TunnelCommand>>>,
    pub generations: Arc<Mutex<HashMap<String, u64>>>,
    pub env_path: Arc<Mutex<String>>,
}

fn stop_command(key: &str, command: &TunnelCommand) -> Result<(), String> {
    info!("Stopping command: {} {:?}", command.command, command.args);
    let mut child = Command::new(&command.kill_command)
        .args(&command.kill_args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start stop command for tunnel '{key}': {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                debug!("Tunnel '{key}' stopped successfully");
                return Ok(());
            }
            Ok(Some(status)) => {
                return Err(format!(
                    "Stop command for tunnel '{key}' exited with status {status}"
                ));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Stop command for tunnel '{key}' timed out"));
            }
            Err(e) => {
                return Err(format!(
                    "Failed to wait for tunnel '{key}' stop command: {e}"
                ));
            }
        }
    }
}

impl TunnelManager {
    /// Toggles a tunnel by name (command_key) on or off.
    /// If turning on, spawns a thread to run the SSH command.
    /// If turning off, kills the process.
    /// Toggle a tunnel on/off. Returns `true` if any tunnels are active after the toggle.
    pub fn toggle(&self, command_key: &str, enable: bool) -> bool {
        if enable {
            let command = {
                let config = self.commands_config.lock().unwrap();
                config.get(command_key).cloned()
            };
            let Some(command) = command else {
                warn!("No command configuration found while starting '{command_key}'");
                return self.has_active_tunnels();
            };

            let generation = {
                let mut generations = self.generations.lock().unwrap();
                let generation = generations.entry(command_key.to_owned()).or_default();
                *generation += 1;
                *generation
            };
            self.active_tunnels
                .lock()
                .unwrap()
                .insert(command_key.to_owned());
            self.active_commands
                .lock()
                .unwrap()
                .insert(command_key.to_owned(), command.clone());

            let active_tunnels = self.active_tunnels.clone();
            let generations = self.generations.clone();
            let command_key = command_key.to_owned();
            let env_path = self.env_path.lock().unwrap().clone();

            thread::spawn(move || {
                let mut attempts = 0;

                // Define closure to check if tunnel is still active
                let is_active = || {
                    active_tunnels.lock().unwrap().contains(&command_key)
                        && generations
                            .lock()
                            .unwrap()
                            .get(&command_key)
                            .is_some_and(|current| *current == generation)
                };

                while is_active() && attempts < 5 {
                    info!(
                        "Spawning command: {} {:?} (attempt {})",
                        command.command, command.args, attempts
                    );

                    let mut cmd = Command::new(&command.command);

                    // PATH to use for subprocesses (provided by platform/app)
                    let config_path = env_path.clone();

                    // Update PATH to include configured paths
                    let new_path = cmd
                        .get_envs()
                        .find(|(key, _)| key == &OsStr::new("PATH"))
                        .and_then(|(_, value)| {
                            value.map(|path| format!("{}:{}", config_path, path.to_string_lossy()))
                        })
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
            self.active_tunnels.lock().unwrap().remove(command_key);
            let mut generations = self.generations.lock().unwrap();
            *generations.entry(command_key.to_owned()).or_default() += 1;
            drop(generations);

            let command = self
                .active_commands
                .lock()
                .unwrap()
                .remove(command_key)
                .or_else(|| {
                    self.commands_config
                        .lock()
                        .unwrap()
                        .get(command_key)
                        .cloned()
                });

            if let Some(command) = command {
                if let Err(e) = stop_command(command_key, &command) {
                    error!("{e}");
                }
            } else {
                warn!("No command configuration found while stopping '{command_key}'");
            }
        }

        self.has_active_tunnels()
    }

    /// Apply new definitions, restarting only active tunnels affected by the change.
    pub fn reconfigure(&self, commands: HashMap<String, TunnelCommand>, env_path: String) {
        let path_changed = *self.env_path.lock().unwrap() != env_path;
        let active_commands = self.active_commands.lock().unwrap().clone();
        let affected: Vec<String> = active_commands
            .iter()
            .filter(|(key, active_command)| {
                path_changed || commands.get(*key) != Some(*active_command)
            })
            .map(|(key, _)| key.clone())
            .collect();

        for key in &affected {
            let mut generations = self.generations.lock().unwrap();
            *generations.entry(key.clone()).or_default() += 1;
        }

        for key in &affected {
            let Some(active_command) = active_commands.get(key) else {
                continue;
            };
            if let Err(e) = stop_command(key, active_command) {
                error!("Config reload could not restart tunnel '{key}': {e}");
                continue;
            }
            self.active_tunnels.lock().unwrap().remove(key);
            self.active_commands.lock().unwrap().remove(key);
        }

        *self.commands_config.lock().unwrap() = commands;
        *self.env_path.lock().unwrap() = env_path;

        for key in affected {
            if !self.active_tunnels.lock().unwrap().contains(&key)
                && self.commands_config.lock().unwrap().contains_key(&key)
            {
                self.toggle(&key, true);
            }
        }
    }

    /// Cleans up all tunnels when the app terminates.
    pub fn cleanup(&self) {
        let active_commands = self.active_commands.lock().unwrap().clone();
        let mut active = self.active_tunnels.lock().unwrap();

        for key in active.iter() {
            debug!("Cleaning up tunnel: {}", key);
            if let Some(command) = active_commands.get(key)
                && let Err(e) = stop_command(key, command)
            {
                error!("{e}");
            }
        }

        // Clear all active
        active.clear();
        self.active_commands.lock().unwrap().clear();
        debug!("All tunnels cleaned up");
    }

    /// Restart all tunnels currently marked as active. Useful after system wake.
    pub fn restart_active_tunnels(&self) {
        // Snapshot active tunnel keys to avoid holding the lock while restarting.
        let active_keys: Vec<String> = {
            let tunnels = self.active_tunnels.lock().unwrap();
            tunnels.iter().cloned().collect()
        };

        if active_keys.is_empty() {
            info!("No active tunnels to restart after wake");
            return;
        }

        info!(
            "Restarting {} active tunnel(s) after wake: {:?}",
            active_keys.len(),
            active_keys
        );

        for key in active_keys {
            // Stop then start each tunnel to re-establish connections after sleep.
            let _ = self.toggle(&key, false);
            thread::sleep(Duration::from_millis(150));
            let _ = self.toggle(&key, true);
        }
    }
}

impl TunnelManager {
    pub fn has_active_tunnels(&self) -> bool {
        let tunnels = self.active_tunnels.lock().unwrap();
        !tunnels.is_empty()
    }
}
