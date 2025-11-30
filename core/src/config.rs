//! Configuration loading and management.
//! Uses injected `AppPaths` so platform shells control where files live.

use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use crate::platform::AppPaths;
use crate::tunnel::TunnelCommand;

// Helper struct for serialization to maintain TOML structure
#[derive(Serialize)]
struct ConfigForSerialization {
    tunnels: HashMap<String, TunnelConfig>,
    schedules: HashMap<String, ScheduledTaskConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
    #[serde(default)]
    pub separator_after: Option<bool>,
    #[serde(default)]
    pub group_header: Option<String>,
    #[serde(default)]
    pub group_icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTaskConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cron_schedule: String,
    #[serde(default)]
    pub separator_after: Option<bool>,
    #[serde(default)]
    pub group_header: Option<String>,
    #[serde(default)]
    pub group_icon: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub tunnels: Vec<(String, TunnelConfig)>,
    #[serde(default)]
    pub schedules: Vec<(String, ScheduledTaskConfig)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Config {
    /// Load configuration from the provided paths. Creates a default file if missing.
    pub fn load_with(paths: &dyn AppPaths) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = paths.config_path();

        if !config_path.exists() {
            info!(
                "Config file not found at {:?}, creating default config",
                config_path
            );
            let default_config = Self::default();
            default_config.save_with(paths)?;
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

    /// Save configuration to the provided paths.
    pub fn save_with(&self, paths: &dyn AppPaths) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = paths.config_path();

        // Create the directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Convert Vec back to HashMap for serialization
        let tunnels_map: std::collections::HashMap<String, TunnelConfig> =
            self.tunnels.iter().cloned().collect();
        let schedules_map: std::collections::HashMap<String, ScheduledTaskConfig> =
            self.schedules.iter().cloned().collect();
        let serializable_config = ConfigForSerialization {
            tunnels: tunnels_map,
            schedules: schedules_map,
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
    /// Return configured PATH or fall back to current process PATH (cross-platform).
    pub fn get_path(&self) -> String {
        if let Some(path) = &self.path {
            return path.clone();
        }
        std::env::var("PATH").unwrap_or_default()
    }

    /// Convert from toml::Value preserving order
    fn from_toml_value(value: toml::Value) -> Result<Self, Box<dyn std::error::Error>> {
        let table = value.as_table().ok_or("Root must be a table")?;

        let path = table
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut tunnels = Vec::new();

        if let Some(tunnels_value) = table.get("tunnels")
            && let Some(tunnels_table) = tunnels_value.as_table()
        {
            // With preserve_order feature, this iteration maintains order
            for (key, value) in tunnels_table {
                let tunnel_config: TunnelConfig = value.clone().try_into()?;
                tunnels.push((key.clone(), tunnel_config));
            }
        }

        let mut schedules = Vec::new();

        if let Some(tasks_value) = table.get("schedules")
            && let Some(tasks_table) = tasks_value.as_table()
        {
            // With preserve_order feature, this iteration maintains order
            for (key, value) in tasks_table {
                let task_config: ScheduledTaskConfig = value.clone().try_into()?;
                schedules.push((key.clone(), task_config));
            }
        }

        Ok(Config {
            tunnels,
            schedules,
            path,
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
                    separator_after: None,
                    group_header: None,
                    group_icon: None,
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
                    separator_after: None,
                    group_header: None,
                    group_icon: None,
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
                    separator_after: None,
                    group_header: None,
                    group_icon: None,
                },
            ),
        ];

        let schedules = vec![
            // Example scheduled task - daily backup at 6:00 AM
            (
                "daily-backup".to_string(),
                ScheduledTaskConfig {
                    name: "Daily Backup".to_string(),
                    command: "echo".to_string(),
                    args: vec!["Running daily backup...".to_string()],
                    cron_schedule: "0 6 * * *".to_string(),
                    separator_after: None,
                    group_header: Some("Scheduled Tasks".to_string()),
                    group_icon: Some("sf:clock.fill".to_string()),
                },
            ),
        ];

        Self {
            tunnels,
            schedules,
            path: None,
        }
    }
}
