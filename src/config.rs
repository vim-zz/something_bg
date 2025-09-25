// src/config.rs
//
// Configuration loading and management for Something in the Background.
// Handles loading tunnel configurations from TOML files.

use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::tunnel::TunnelCommand;

// Helper struct for serialization to maintain TOML structure
#[derive(Serialize)]
struct ConfigForSerialization {
    tunnels: HashMap<String, TunnelConfig>,
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub tunnels: Vec<(String, TunnelConfig)>,
    pub path: Option<String>,
}

impl Config {
    /// Load configuration from the default location (~/.config/something_bg/config.toml)
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = get_config_path()?;

        if !config_path.exists() {
            info!(
                "Config file not found at {:?}, creating default config",
                config_path
            );
            let default_config = Self::default();
            default_config.save()?;
            return Ok(default_config);
        }

        debug!("Loading config from {:?}", config_path);
        let content = fs::read_to_string(&config_path)?;

        // Parse as toml::Value first to preserve order, then convert
        let value: toml::Value = content.parse()?;
        let config = Self::from_toml_value(value)?;

        info!("Loaded {} tunnel configurations", config.tunnels.len());
        Ok(config)
    }

    /// Convert from toml::Value preserving order
    fn from_toml_value(value: toml::Value) -> Result<Self, Box<dyn std::error::Error>> {
        let table = value.as_table().ok_or("Root must be a table")?;

        let path = table
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut tunnels = Vec::new();

        if let Some(tunnels_value) = table.get("tunnels") {
            if let Some(tunnels_table) = tunnels_value.as_table() {
                // With preserve_order feature, this iteration maintains order
                for (key, value) in tunnels_table {
                    let tunnel_config: TunnelConfig = value.clone().try_into()?;
                    tunnels.push((key.clone(), tunnel_config));
                }
            }
        }

        Ok(Config { tunnels, path })
    }

    /// Save configuration to the default location
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = get_config_path()?;

        // Create the directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Convert Vec back to HashMap for serialization
        let tunnels_map: std::collections::HashMap<String, TunnelConfig> =
            self.tunnels.iter().cloned().collect();
        let serializable_config = ConfigForSerialization {
            tunnels: tunnels_map,
            path: self.path.clone(),
        };
        let content = toml::to_string_pretty(&serializable_config)?;
        fs::write(&config_path, content)?;

        info!("Saved config to {:?}", config_path);
        Ok(())
    }

    /// Convert tunnel configs to the format expected by TunnelManager
    pub fn to_tunnel_commands(&self) -> HashMap<String, TunnelCommand> {
        self.tunnels
            .iter()
            .map(|(key, config)| {
                (
                    key.clone(),
                    TunnelCommand {
                        command: config.command.clone(),
                        args: config.args.clone(),
                        kill_command: config.kill_command.clone(),
                        kill_args: config.kill_args.clone(),
                    },
                )
            })
            .collect()
    }

    /// Get the configured PATH or return default
    pub fn get_path(&self) -> String {
        self.path.clone().unwrap_or_else(|| {
            "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/opt/homebrew/bin".to_string()
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        let tunnels = vec![
            // Example SSH tunnel configurations
            (
                "example-ssh".to_string(),
                TunnelConfig {
                    name: "Example SSH Tunnel".to_string(),
                    command: "ssh".to_string(),
                    args: vec![
                        "-N".to_string(),
                        "-L".to_string(),
                        "5432:localhost:5432".to_string(),
                        "user@example.com".to_string(),
                    ],
                    kill_command: "pkill".to_string(),
                    kill_args: vec!["-f".to_string(), "user@example.com".to_string()],
                },
            ),
            // Example Kubernetes port forward
            (
                "k8s-example".to_string(),
                TunnelConfig {
                    name: "K8s Port Forward".to_string(),
                    command: "kubectl".to_string(),
                    args: vec![
                        "port-forward".to_string(),
                        "svc/my-service".to_string(),
                        "8080:8080".to_string(),
                        "-n".to_string(),
                        "default".to_string(),
                    ],
                    kill_command: "pkill".to_string(),
                    kill_args: vec!["-f".to_string(), "svc/my-service".to_string()],
                },
            ),
            // Docker environment management
            (
                "colima".to_string(),
                TunnelConfig {
                    name: "Colima Docker".to_string(),
                    command: "colima".to_string(),
                    args: vec!["start".to_string()],
                    kill_command: "colima".to_string(),
                    kill_args: vec!["stop".to_string()],
                },
            ),
        ];

        Self {
            tunnels,
            path: Some(
                "/bin:/usr/bin:/usr/local/bin:/sbin:/usr/sbin:/opt/homebrew/bin".to_string(),
            ),
        }
    }
}

/// Get the path to the config file (~/.config/something_bg/config.toml)
fn get_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home_dir = std::env::var("HOME").map_err(|_| "HOME environment variable not set")?;

    let config_path = PathBuf::from(home_dir)
        .join(".config")
        .join("something_bg")
        .join("config.toml");

    Ok(config_path)
}
