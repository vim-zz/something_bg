//! Versioned configuration loading, migration, and runtime models.
//! Uses injected `AppPaths` so platform shells control where files live.

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::platform::AppPaths;
use crate::tunnel::TunnelCommand;

pub const CURRENT_CONFIG_VERSION: u64 = 2;

/// Tracks the exact config contents that were last applied by the app.
pub struct ConfigMonitor {
    path: PathBuf,
    applied_contents: Mutex<Option<Vec<u8>>>,
}

impl ConfigMonitor {
    pub fn new(path: PathBuf, applied_contents: Option<Vec<u8>>) -> Self {
        Self {
            path,
            applied_contents: Mutex::new(applied_contents),
        }
    }

    pub fn has_changed(&self) -> std::io::Result<bool> {
        let current = match fs::read(&self.path) {
            Ok(contents) => Some(contents),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let applied = self.applied_contents.lock().unwrap();
        Ok(*applied != current)
    }

    pub fn mark_setting_applied(&self, previous: &[u8], contents: Vec<u8>) {
        let mut applied = self.applied_contents.lock().unwrap();
        if applied.as_deref() == Some(previous) {
            *applied = Some(contents);
        }
    }

    pub fn mark_applied(&self, contents: Vec<u8>) {
        *self.applied_contents.lock().unwrap() = Some(contents);
    }
}

/// A targeted config edit, prepared before changing an OS preference.
/// Keeps comments, unknown keys, and unapplied manual edits intact.
pub struct LoginSettingUpdate {
    path: PathBuf,
    pub original: Vec<u8>,
    pub updated: Vec<u8>,
}

impl LoginSettingUpdate {
    pub fn prepare(
        paths: &dyn AppPaths,
        enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = fs::canonicalize(paths.config_path())?;
        let original = fs::read(&path)?;
        let content = std::str::from_utf8(&original)?;
        let value: toml::Value = content.parse()?;
        if declared_version(&value)? != CURRENT_CONFIG_VERSION {
            return Err("Reload the configuration before changing Start at Login.".into());
        }
        // Validate the whole config without rewriting or applying it.
        Config::from_v2_document(value.try_into()?)?;
        let mut document: toml_edit::Document = content.parse()?;
        let item = &mut document["settings"]["start_at_login"];
        let mut value = toml_edit::Value::from(enabled);
        if let Some(previous) = item.as_value() {
            *value.decor_mut() = previous.decor().clone();
        }
        *item = toml_edit::Item::Value(value);
        Ok(Self {
            path,
            original,
            updated: document.to_string().into_bytes(),
        })
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::Write;
        // Do not clobber an editor's intervening changes.
        if fs::read(&self.path)? != self.original {
            return Err(
                "Configuration changed while saving Start at Login. Please try again.".into(),
            );
        }
        let temp_path = self
            .path
            .with_extension(format!("toml.{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        let result = (|| -> std::io::Result<()> {
            file.set_permissions(fs::metadata(&self.path)?.permissions())?;
            file.write_all(&self.updated)?;
            file.sync_all()?;
            fs::rename(&temp_path, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SectionKind {
    Tunnel,
    Command,
    ScheduledTask,
}

#[derive(Debug, Clone)]
pub struct ConfigSection {
    pub id: String,
    pub title: Option<String>,
    pub icon: Option<String>,
    pub kind: SectionKind,
    /// False renders the section flush with the one above it, with no divider.
    pub separator: bool,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TunnelConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub kill_command: String,
    pub kill_args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScheduledTaskConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cron_schedule: String,
}

#[derive(Debug, Clone)]
pub struct CommandConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub output: Option<String>,
}

#[derive(Debug)]
pub struct Config {
    pub settings: Settings,
    pub sections: Vec<ConfigSection>,
    pub tunnels: Vec<(String, TunnelConfig)>,
    pub commands: Vec<(String, CommandConfig)>,
    pub schedules: Vec<(String, ScheduledTaskConfig)>,
    pub scripts_dir: Option<String>,
    pub scripts_output: Option<String>,
    pub path: Option<String>,
    scripts_section: Option<String>,
    discovered_command_ids: HashSet<String>,
}

/// App preferences. Platform-specific settings are retained on every platform.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub start_at_login: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EnvironmentDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScriptsDocument {
    directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct V2Document {
    version: u64,
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    environment: EnvironmentDocument,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scripts: Option<ScriptsDocument>,
    #[serde(default)]
    sections: Vec<SectionDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SectionDocument {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    kind: SectionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    separator: Option<bool>,
    #[serde(default)]
    items: Vec<ItemDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ItemDocument {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyTunnelConfig {
    name: String,
    command: String,
    args: Vec<String>,
    kill_command: String,
    kill_args: Vec<String>,
    #[serde(default)]
    separator_after: Option<bool>,
    #[serde(default)]
    group_header: Option<String>,
    #[serde(default)]
    group_icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyScheduledTaskConfig {
    name: String,
    command: String,
    args: Vec<String>,
    cron_schedule: String,
    #[serde(default)]
    separator_after: Option<bool>,
    #[serde(default)]
    group_header: Option<String>,
    #[serde(default)]
    group_icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyCommandConfig {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    separator_after: Option<bool>,
    #[serde(default)]
    group_header: Option<String>,
    #[serde(default)]
    group_icon: Option<String>,
}

impl Config {
    pub fn load_with(paths: &dyn AppPaths) -> Result<Self, Box<dyn std::error::Error>> {
        Self::load_with_snapshot(paths).map(|(config, _)| config)
    }

    pub fn load_with_snapshot(
        paths: &dyn AppPaths,
    ) -> Result<(Self, Vec<u8>), Box<dyn std::error::Error>> {
        let config_path = paths.config_path();

        if !config_path.exists() {
            info!(
                "Config file not found at {:?}, creating v2 config",
                config_path
            );
            Self::default().save_with(paths)?;
        }

        debug!("Loading config from {:?}", config_path);
        let original_contents = fs::read(&config_path)?;
        let content = std::str::from_utf8(&original_contents)?;
        let value: toml::Value = content.parse()?;
        let version = declared_version(&value)?;

        let (config, applied_contents) = match version {
            1 => {
                let document = migrate_v1_to_v2(value)?;
                let config = Self::from_v2_document(document.clone())?;
                let migrated = toml::to_string_pretty(&document)?.into_bytes();
                persist_migration(&config_path, &original_contents, &migrated, 1)?;
                info!(
                    "Migrated configuration from v1 to v{}; backup saved next to config",
                    CURRENT_CONFIG_VERSION
                );
                (config, migrated)
            }
            CURRENT_CONFIG_VERSION => {
                let document: V2Document = value.try_into()?;
                (Self::from_v2_document(document)?, original_contents)
            }
            other => {
                return Err(format!(
                    "Unsupported config version {other}; this app supports up to version {CURRENT_CONFIG_VERSION}"
                )
                .into());
            }
        };

        info!(
            "Loaded {} tunnels, {} commands, and {} scheduled tasks",
            config.tunnels.len(),
            config.commands.len(),
            config.schedules.len()
        );
        Ok((config, applied_contents))
    }

    pub fn save_with(&self, paths: &dyn AppPaths) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = paths.config_path();
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&self.to_v2_document())?;
        fs::write(&config_path, content)?;
        info!(
            "Saved v{} config to {:?}",
            CURRENT_CONFIG_VERSION, config_path
        );
        Ok(())
    }

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

    pub fn get_path(&self) -> String {
        self.path
            .clone()
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default())
    }

    pub fn tunnel(&self, id: &str) -> Option<&TunnelConfig> {
        self.tunnels
            .iter()
            .find_map(|(key, config)| (key == id).then_some(config))
    }

    pub fn command(&self, id: &str) -> Option<&CommandConfig> {
        self.commands
            .iter()
            .find_map(|(key, config)| (key == id).then_some(config))
    }

    pub fn schedule(&self, id: &str) -> Option<&ScheduledTaskConfig> {
        self.schedules
            .iter()
            .find_map(|(key, config)| (key == id).then_some(config))
    }

    fn from_v2_document(document: V2Document) -> Result<Self, Box<dyn std::error::Error>> {
        if document.version != CURRENT_CONFIG_VERSION {
            return Err(format!("Expected config version {CURRENT_CONFIG_VERSION}").into());
        }

        let scripts_dir = document.scripts.as_ref().map(|s| s.directory.clone());
        let scripts_output = document.scripts.as_ref().and_then(|s| s.output.clone());
        let scripts_section = document.scripts.as_ref().and_then(|s| s.section.clone());
        let mut config = Config {
            settings: document.settings,
            sections: Vec::new(),
            tunnels: Vec::new(),
            commands: Vec::new(),
            schedules: Vec::new(),
            scripts_dir,
            scripts_output,
            path: document.environment.path,
            scripts_section,
            discovered_command_ids: HashSet::new(),
        };

        let mut section_ids = HashSet::new();
        let mut tunnel_ids = HashSet::new();
        let mut command_ids = HashSet::new();
        let mut schedule_ids = HashSet::new();

        for section in document.sections {
            if !section_ids.insert(section.id.clone()) {
                return Err(format!("Duplicate section id '{}'", section.id).into());
            }

            let mut item_ids = Vec::new();
            for item in section.items {
                let id = item.id.clone();
                match section.kind {
                    SectionKind::Tunnel => {
                        if !tunnel_ids.insert(id.clone()) {
                            return Err(format!("Duplicate tunnel id '{id}'").into());
                        }
                        let (command, args) = split_action(item.start, "start", &id)?;
                        let (kill_command, kill_args) = split_action(item.stop, "stop", &id)?;
                        config.tunnels.push((
                            id.clone(),
                            TunnelConfig {
                                name: item.name,
                                command,
                                args,
                                kill_command,
                                kill_args,
                            },
                        ));
                    }
                    SectionKind::Command => {
                        if !command_ids.insert(id.clone()) {
                            return Err(format!("Duplicate command id '{id}'").into());
                        }
                        let (command, args) = split_action(item.run, "run", &id)?;
                        config.commands.push((
                            id.clone(),
                            CommandConfig {
                                name: item.name,
                                command,
                                args,
                                output: item.output,
                            },
                        ));
                    }
                    SectionKind::ScheduledTask => {
                        if !schedule_ids.insert(id.clone()) {
                            return Err(format!("Duplicate scheduled-task id '{id}'").into());
                        }
                        let (command, args) = split_action(item.run, "run", &id)?;
                        let cron_schedule = item
                            .cron
                            .ok_or_else(|| format!("Scheduled task '{id}' requires 'cron'"))?;
                        config.schedules.push((
                            id.clone(),
                            ScheduledTaskConfig {
                                name: item.name,
                                command,
                                args,
                                cron_schedule,
                            },
                        ));
                    }
                }
                item_ids.push(id);
            }

            config.sections.push(ConfigSection {
                id: section.id,
                title: section.title,
                icon: section.icon,
                kind: section.kind,
                separator: section.separator.unwrap_or(true),
                item_ids,
            });
        }

        if let Some(scripts) = document.scripts {
            config.add_discovered_scripts(&scripts)?;
        }
        Ok(config)
    }

    fn add_discovered_scripts(
        &mut self,
        scripts: &ScriptsDocument,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let discovered = discover_scripts(&scripts.directory, scripts.output.as_deref());
        if discovered.is_empty() {
            return Ok(());
        }

        let requested_id = scripts.section.as_deref().unwrap_or("scripts");
        let section_index = if let Some(index) = self
            .sections
            .iter()
            .position(|section| section.id == requested_id)
        {
            if self.sections[index].kind != SectionKind::Command {
                return Err(format!(
                    "Scripts section '{}' must have kind = 'command'",
                    requested_id
                )
                .into());
            }
            index
        } else {
            self.sections.push(ConfigSection {
                id: requested_id.to_string(),
                title: Some("Scripts".to_string()),
                icon: Some("sf:terminal.fill".to_string()),
                kind: SectionKind::Command,
                separator: true,
                item_ids: Vec::new(),
            });
            self.sections.len() - 1
        };

        let existing: HashSet<_> = self.commands.iter().map(|(id, _)| id.clone()).collect();
        for (id, command) in discovered {
            if existing.contains(&id) {
                warn!("Skipping discovered script with duplicate command id '{id}'");
                continue;
            }
            self.sections[section_index].item_ids.push(id.clone());
            self.discovered_command_ids.insert(id.clone());
            self.commands.push((id, command));
        }
        Ok(())
    }

    fn to_v2_document(&self) -> V2Document {
        let sections = self
            .sections
            .iter()
            .map(|section| {
                let items = section
                    .item_ids
                    .iter()
                    .filter(|id| !self.discovered_command_ids.contains(*id))
                    .filter_map(|id| match section.kind {
                        SectionKind::Tunnel => self.tunnel(id).map(|config| ItemDocument {
                            id: id.clone(),
                            name: config.name.clone(),
                            start: Some(join_action(&config.command, &config.args)),
                            stop: Some(join_action(&config.kill_command, &config.kill_args)),
                            run: None,
                            cron: None,
                            output: None,
                        }),
                        SectionKind::Command => self.command(id).map(|config| ItemDocument {
                            id: id.clone(),
                            name: config.name.clone(),
                            start: None,
                            stop: None,
                            run: Some(join_action(&config.command, &config.args)),
                            cron: None,
                            output: config.output.clone(),
                        }),
                        SectionKind::ScheduledTask => {
                            self.schedule(id).map(|config| ItemDocument {
                                id: id.clone(),
                                name: config.name.clone(),
                                start: None,
                                stop: None,
                                run: Some(join_action(&config.command, &config.args)),
                                cron: Some(config.cron_schedule.clone()),
                                output: None,
                            })
                        }
                    })
                    .collect();
                SectionDocument {
                    id: section.id.clone(),
                    title: section.title.clone(),
                    icon: section.icon.clone(),
                    kind: section.kind,
                    separator: if section.separator { None } else { Some(false) },
                    items,
                }
            })
            .collect();

        V2Document {
            version: CURRENT_CONFIG_VERSION,
            settings: self.settings.clone(),
            environment: EnvironmentDocument {
                path: self.path.clone(),
            },
            scripts: self.scripts_dir.as_ref().map(|directory| ScriptsDocument {
                directory: directory.clone(),
                output: self.scripts_output.clone(),
                section: self.scripts_section.clone(),
            }),
            sections,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_v2_document(V2Document {
            version: CURRENT_CONFIG_VERSION,
            settings: Settings::default(),
            environment: EnvironmentDocument::default(),
            scripts: None,
            sections: vec![
                SectionDocument {
                    id: "connections".to_string(),
                    title: Some("Connections".to_string()),
                    icon: Some("sf:cylinder.fill".to_string()),
                    kind: SectionKind::Tunnel,
                    separator: None,
                    items: vec![
                        ItemDocument {
                            id: "example-ssh".to_string(),
                            name: "Example SSH Tunnel".to_string(),
                            start: Some(vec![
                                "ssh".to_string(),
                                "-N".to_string(),
                                "-L".to_string(),
                                "5432:localhost:5432".to_string(),
                                "user@example.com".to_string(),
                            ]),
                            stop: Some(vec![
                                "pkill".to_string(),
                                "-f".to_string(),
                                "user@example.com".to_string(),
                            ]),
                            run: None,
                            cron: None,
                            output: None,
                        },
                        ItemDocument {
                            id: "k8s-example".to_string(),
                            name: "K8s Port Forward".to_string(),
                            start: Some(vec![
                                "kubectl".to_string(),
                                "port-forward".to_string(),
                                "svc/my-service".to_string(),
                                "8080:8080".to_string(),
                                "-n".to_string(),
                                "default".to_string(),
                            ]),
                            stop: Some(vec![
                                "pkill".to_string(),
                                "-f".to_string(),
                                "svc/my-service".to_string(),
                            ]),
                            run: None,
                            cron: None,
                            output: None,
                        },
                    ],
                },
                SectionDocument {
                    id: "services".to_string(),
                    title: Some("Services".to_string()),
                    icon: Some("sf:ferry".to_string()),
                    kind: SectionKind::Tunnel,
                    separator: None,
                    items: vec![ItemDocument {
                        id: "colima".to_string(),
                        name: "Colima Docker".to_string(),
                        start: Some(vec!["colima".to_string(), "start".to_string()]),
                        stop: Some(vec!["colima".to_string(), "stop".to_string()]),
                        run: None,
                        cron: None,
                        output: None,
                    }],
                },
                SectionDocument {
                    id: "scheduled".to_string(),
                    title: Some("Scheduled Tasks".to_string()),
                    icon: Some("sf:clock.fill".to_string()),
                    kind: SectionKind::ScheduledTask,
                    separator: None,
                    items: vec![ItemDocument {
                        id: "daily-backup".to_string(),
                        name: "Daily Backup".to_string(),
                        start: None,
                        stop: None,
                        run: Some(vec![
                            "echo".to_string(),
                            "Running daily backup...".to_string(),
                        ]),
                        cron: Some("0 6 * * *".to_string()),
                        output: None,
                    }],
                },
            ],
        })
        .expect("built-in v2 config must be valid")
    }
}

fn declared_version(value: &toml::Value) -> Result<u64, Box<dyn std::error::Error>> {
    let table = value.as_table().ok_or("Root must be a table")?;
    match table.get("version") {
        None => Ok(1),
        Some(value) => value
            .as_integer()
            .and_then(|version| u64::try_from(version).ok())
            .ok_or_else(|| "Config 'version' must be a positive integer".into()),
    }
}

fn split_action(
    action: Option<Vec<String>>,
    field: &str,
    item_id: &str,
) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    let mut action = action.ok_or_else(|| format!("Item '{item_id}' requires '{field}'"))?;
    if action.is_empty() {
        return Err(format!("Item '{item_id}' has an empty '{field}' command").into());
    }
    let command = action.remove(0);
    if command.is_empty() {
        return Err(format!("Item '{item_id}' has an empty executable in '{field}'").into());
    }
    Ok((command, action))
}

fn join_action(command: &str, args: &[String]) -> Vec<String> {
    std::iter::once(command.to_string())
        .chain(args.iter().cloned())
        .collect()
}

fn persist_migration(
    config_path: &Path,
    original: &[u8],
    migrated: &[u8],
    from_version: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.toml");
    let backup_path = config_path.with_file_name(format!("{file_name}.v{from_version}.bak"));
    if !backup_path.exists() {
        fs::write(&backup_path, original)?;
    }
    fs::write(config_path, migrated)?;
    Ok(())
}

/// Permanent v1 adapter. Do not remove when newer schemas are introduced; future
/// migrations should chain this document through each subsequent version.
fn migrate_v1_to_v2(value: toml::Value) -> Result<V2Document, Box<dyn std::error::Error>> {
    let table = value.as_table().ok_or("Root must be a table")?;
    let path = optional_string(table, "path");
    let scripts_dir = optional_string(table, "scripts_dir");
    let scripts_output = optional_string(table, "scripts_output");
    let mut sections = Vec::new();
    let mut used_section_ids = HashSet::new();

    let tunnels: Vec<_> = legacy_entries::<LegacyTunnelConfig>(table, "tunnels")?;
    sections.extend(migrate_legacy_tunnels(tunnels, &mut used_section_ids));

    let commands: Vec<_> = legacy_entries::<LegacyCommandConfig>(table, "commands")?;
    sections.extend(migrate_legacy_commands(commands, &mut used_section_ids));

    let scripts = scripts_dir.map(|directory| {
        let section = unique_section_id("scripts", &mut used_section_ids);
        sections.push(SectionDocument {
            id: section.clone(),
            title: Some("Scripts".to_string()),
            icon: Some("sf:terminal.fill".to_string()),
            kind: SectionKind::Command,
            separator: None,
            items: Vec::new(),
        });
        ScriptsDocument {
            directory,
            output: scripts_output,
            section: Some(section),
        }
    });

    let schedules: Vec<_> = legacy_entries::<LegacyScheduledTaskConfig>(table, "schedules")?;
    sections.extend(migrate_legacy_schedules(schedules, &mut used_section_ids));

    Ok(V2Document {
        version: CURRENT_CONFIG_VERSION,
        settings: table
            .get("settings")
            .cloned()
            .map(toml::Value::try_into)
            .transpose()?
            .unwrap_or_default(),
        environment: EnvironmentDocument { path },
        scripts,
        sections,
    })
}

fn optional_string(table: &toml::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

fn legacy_entries<T>(
    root: &toml::Table,
    key: &str,
) -> Result<Vec<(String, T)>, Box<dyn std::error::Error>>
where
    T: for<'de> Deserialize<'de>,
{
    let Some(value) = root.get(key) else {
        return Ok(Vec::new());
    };
    let table = value
        .as_table()
        .ok_or_else(|| format!("Legacy '{key}' must be a table"))?;
    table
        .iter()
        .map(|(id, value)| Ok((id.clone(), value.clone().try_into()?)))
        .collect()
}

fn migrate_legacy_tunnels(
    entries: Vec<(String, LegacyTunnelConfig)>,
    used: &mut HashSet<String>,
) -> Vec<SectionDocument> {
    migrate_legacy_entries(entries, SectionKind::Tunnel, "tunnels", used, |id, item| {
        ItemDocument {
            id,
            name: item.name,
            start: Some(join_action(&item.command, &item.args)),
            stop: Some(join_action(&item.kill_command, &item.kill_args)),
            run: None,
            cron: None,
            output: None,
        }
    })
}

fn migrate_legacy_commands(
    entries: Vec<(String, LegacyCommandConfig)>,
    used: &mut HashSet<String>,
) -> Vec<SectionDocument> {
    migrate_legacy_entries(
        entries,
        SectionKind::Command,
        "commands",
        used,
        |id, item| ItemDocument {
            id,
            name: item.name,
            start: None,
            stop: None,
            run: Some(join_action(&item.command, &item.args)),
            cron: None,
            output: item.output,
        },
    )
}

fn migrate_legacy_schedules(
    entries: Vec<(String, LegacyScheduledTaskConfig)>,
    used: &mut HashSet<String>,
) -> Vec<SectionDocument> {
    migrate_legacy_entries(
        entries,
        SectionKind::ScheduledTask,
        "scheduled",
        used,
        |id, item| ItemDocument {
            id,
            name: item.name,
            start: None,
            stop: None,
            run: Some(join_action(&item.command, &item.args)),
            cron: Some(item.cron_schedule),
            output: None,
        },
    )
}

trait LegacyPresentation {
    fn group_header(&self) -> Option<&str>;
    fn group_icon(&self) -> Option<&str>;
    fn separator_after(&self) -> bool;
}

macro_rules! impl_legacy_presentation {
    ($type:ty) => {
        impl LegacyPresentation for $type {
            fn group_header(&self) -> Option<&str> {
                self.group_header.as_deref()
            }
            fn group_icon(&self) -> Option<&str> {
                self.group_icon.as_deref()
            }
            fn separator_after(&self) -> bool {
                self.separator_after.unwrap_or(false)
            }
        }
    };
}

impl_legacy_presentation!(LegacyTunnelConfig);
impl_legacy_presentation!(LegacyCommandConfig);
impl_legacy_presentation!(LegacyScheduledTaskConfig);

fn migrate_legacy_entries<T, F>(
    entries: Vec<(String, T)>,
    kind: SectionKind,
    default_id: &str,
    used: &mut HashSet<String>,
    convert: F,
) -> Vec<SectionDocument>
where
    T: LegacyPresentation,
    F: Fn(String, T) -> ItemDocument,
{
    let mut sections = Vec::new();
    let mut current: Option<SectionDocument> = None;

    for (id, item) in entries {
        if let Some(header) = item.group_header() {
            if let Some(section) = current.take()
                && !section.items.is_empty()
            {
                sections.push(section);
            }
            current = Some(SectionDocument {
                id: unique_section_id(&slugify(header, default_id), used),
                title: Some(header.to_string()),
                icon: item.group_icon().map(str::to_string),
                kind,
                separator: None,
                items: Vec::new(),
            });
        }

        if current.is_none() {
            current = Some(SectionDocument {
                id: unique_section_id(default_id, used),
                title: None,
                icon: None,
                kind,
                separator: None,
                items: Vec::new(),
            });
        }

        let separator_after = item.separator_after();
        current.as_mut().unwrap().items.push(convert(id, item));
        if separator_after && let Some(section) = current.take() {
            sections.push(section);
        }
    }

    if let Some(section) = current
        && !section.items.is_empty()
    {
        sections.push(section);
    }
    sections
}

fn slugify(value: &str, fallback: &str) -> String {
    let slug = value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    }
}

fn unique_section_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn expand_tilde(path: &str) -> String {
    if (path == "~" || path.starts_with("~/"))
        && let Some(home) = dirs::home_dir()
    {
        return path.replacen('~', &home.to_string_lossy(), 1);
    }
    path.to_string()
}

fn discover_scripts(dir: &str, output_mode: Option<&str>) -> Vec<(String, CommandConfig)> {
    use crate::scheduler::capitalize_first;

    let expanded = expand_tilde(dir);
    let dir_path = Path::new(&expanded);
    let mut scripts: Vec<_> = match fs::read_dir(dir_path) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sh"))
            .collect(),
        Err(e) => {
            warn!("Failed to read scripts directory {}: {}", dir, e);
            return Vec::new();
        }
    };
    scripts.sort_by_key(|entry| entry.file_name());

    scripts
        .into_iter()
        .map(|entry| {
            let path = entry.path();
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let name = stem
                .split(['-', '_'])
                .map(capitalize_first)
                .collect::<Vec<_>>()
                .join(" ");
            (
                format!("script-{stem}"),
                CommandConfig {
                    name,
                    command: "bash".to_string(),
                    args: vec![path.to_string_lossy().to_string()],
                    output: Some(output_mode.unwrap_or("notify").to_string()),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestPaths {
        directory: PathBuf,
    }

    impl AppPaths for TestPaths {
        fn config_path(&self) -> PathBuf {
            self.directory.join("config.toml")
        }

        fn state_path(&self) -> PathBuf {
            self.directory.join("state.toml")
        }
    }

    fn test_paths(name: &str) -> TestPaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "something-bg-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        TestPaths { directory }
    }

    #[test]
    fn config_monitor_detects_and_acknowledges_content_changes() {
        let paths = test_paths("monitor");
        let path = paths.config_path();
        fs::write(&path, "path = 'first'").unwrap();
        let monitor = ConfigMonitor::new(path.clone(), fs::read(&path).ok());
        assert!(!monitor.has_changed().unwrap());

        fs::write(&path, "path = 'second'").unwrap();
        assert!(monitor.has_changed().unwrap());
        monitor.mark_applied(fs::read(&path).unwrap());
        assert!(!monitor.has_changed().unwrap());

        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn login_preference_defaults_off_and_round_trips() {
        let paths = test_paths("login-default");
        fs::write(paths.config_path(), "version = 2\n").unwrap();
        let mut config = Config::load_with(&paths).unwrap();
        assert!(!config.settings.start_at_login);
        config.settings.start_at_login = true;
        config.save_with(&paths).unwrap();
        assert!(Config::load_with(&paths).unwrap().settings.start_at_login);
        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn login_toggle_preserves_comments_unknown_keys_and_manual_edits() {
        let paths = test_paths("login-comments");
        let original = "# My config\nversion = 2\n\n[settings]\nstart_at_login = false # preference\ncustom = 'keep me'\n\n[environment]\npath = '/custom/bin'\n";
        fs::write(paths.config_path(), original).unwrap();
        let monitor = ConfigMonitor::new(paths.config_path(), Some(original.as_bytes().to_vec()));
        let update = LoginSettingUpdate::prepare(&paths, true).unwrap();
        update.save().unwrap();
        assert_eq!(
            fs::read_to_string(paths.config_path()).unwrap(),
            original.replace("false", "true")
        );
        monitor.mark_setting_applied(&update.original, update.updated);
        assert!(!monitor.has_changed().unwrap());

        let manually_edited = fs::read_to_string(paths.config_path())
            .unwrap()
            .replace("/custom/bin", "/other/bin");
        fs::write(paths.config_path(), &manually_edited).unwrap();
        let update = LoginSettingUpdate::prepare(&paths, false).unwrap();
        update.save().unwrap();
        monitor.mark_setting_applied(&update.original, update.updated);
        assert!(
            monitor.has_changed().unwrap(),
            "Manual edits must still need Reload"
        );
        assert_eq!(
            fs::read_to_string(paths.config_path()).unwrap(),
            manually_edited.replace("true", "false")
        );
        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn login_toggle_inserts_missing_settings_and_handles_inline_table() {
        let paths = test_paths("login-insert");
        for original in [
            "version = 2\n",
            "version = 2\nsettings = { start_at_login = false, custom = 'keep' }\n",
        ] {
            fs::write(paths.config_path(), original).unwrap();
            LoginSettingUpdate::prepare(&paths, true)
                .unwrap()
                .save()
                .unwrap();
            assert!(Config::load_with(&paths).unwrap().settings.start_at_login);
        }
        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn login_toggle_rejects_invalid_or_changed_config_without_overwriting() {
        let paths = test_paths("login-invalid");
        for original in [
            "invalid!",
            "version = 99",
            "version = 2\nsettings = false",
            "version = 2\n[settings]\nstart_at_login = 'yes'",
        ] {
            fs::write(paths.config_path(), original).unwrap();
            assert!(LoginSettingUpdate::prepare(&paths, true).is_err());
            assert_eq!(fs::read_to_string(paths.config_path()).unwrap(), original);
        }
        fs::write(paths.config_path(), "version = 2\n").unwrap();
        let update = LoginSettingUpdate::prepare(&paths, true).unwrap();
        let changed = "version = 2\n# edited while saving\n";
        fs::write(paths.config_path(), changed).unwrap();
        assert!(update.save().is_err());
        assert_eq!(fs::read_to_string(paths.config_path()).unwrap(), changed);
        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn failed_login_save_leaves_original_config_intact() {
        let paths = test_paths("login-save-failure");
        let original = "version = 2\n";
        fs::write(paths.config_path(), original).unwrap();
        let update = LoginSettingUpdate::prepare(&paths, true).unwrap();
        let temp_path = paths
            .config_path()
            .with_extension(format!("toml.{}.tmp", std::process::id()));
        fs::create_dir(&temp_path).unwrap();
        assert!(update.save().is_err());
        assert_eq!(fs::read_to_string(paths.config_path()).unwrap(), original);
        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn loads_v2_sections_in_declared_order() {
        let document = r#"
version = 2

[environment]
path = "/custom/bin"

[[sections]]
id = "utilities"
title = "UTILITIES"
kind = "command"

[[sections.items]]
id = "hello"
name = "Hello"
run = ["echo", "hello"]
output = "notify"

[[sections]]
id = "connections"
title = "CONNECTIONS"
kind = "tunnel"

[[sections.items]]
id = "database"
name = "Database"
start = ["ssh", "-N", "database"]
stop = ["pkill", "-f", "database"]
"#;
        let value: toml::Value = document.parse().unwrap();
        let config = Config::from_v2_document(value.try_into().unwrap()).unwrap();

        assert_eq!(config.sections[0].id, "utilities");
        assert_eq!(config.sections[1].id, "connections");
        assert_eq!(config.commands[0].0, "hello");
        assert_eq!(config.tunnels[0].0, "database");
        assert_eq!(config.get_path(), "/custom/bin");
    }

    #[test]
    fn section_separator_defaults_on_and_round_trips_when_disabled() {
        let document = r#"
version = 2

[[sections]]
id = "clickhouse"
title = "CLICKHOUSE"
kind = "tunnel"

[[sections.items]]
id = "ch"
name = "PROD"
start = ["kubectl", "port-forward"]
stop = ["pkill", "-f", "port-forward"]

[[sections]]
id = "clickhouse-web"
kind = "command"
separator = false

[[sections.items]]
id = "ch-web"
name = "ClickHouse Web"
run = ["open", "http://localhost:8124/play"]
"#;
        let value: toml::Value = document.parse().unwrap();
        let config = Config::from_v2_document(value.try_into().unwrap()).unwrap();

        assert!(config.sections[0].separator);
        assert!(!config.sections[1].separator);

        let rendered = toml::to_string(&config.to_v2_document()).unwrap();
        assert!(!rendered.contains("separator = true"));
        assert_eq!(rendered.matches("separator = false").count(), 1);
    }

    #[test]
    fn migrates_unversioned_v1_and_preserves_backup() {
        let paths = test_paths("migration");
        let legacy = r#"
path = "/legacy/bin"

[tunnels.prod]
name = "RDS prod"
command = "ssh"
args = ["-N", "prod"]
kill_command = "pkill"
kill_args = ["-f", "prod"]
group_header = "CONNECTIONS"
group_icon = "sf:cylinder.fill"

[tunnels.dev]
name = "RDS dev"
command = "ssh"
args = ["-N", "dev"]
kill_command = "pkill"
kill_args = ["-f", "dev"]
separator_after = true

[schedules.backup]
name = "Backup"
command = "backup"
args = []
cron_schedule = "0 10 * * *"
group_header = "SCHEDULED"
"#;
        fs::write(paths.config_path(), legacy).unwrap();

        let (config, snapshot) = Config::load_with_snapshot(&paths).unwrap();
        let rewritten = fs::read_to_string(paths.config_path()).unwrap();
        let backup = paths.directory.join("config.toml.v1.bak");

        assert!(rewritten.starts_with("version = 2"));
        assert_eq!(snapshot, rewritten.as_bytes());
        assert_eq!(fs::read_to_string(&backup).unwrap(), legacy);
        assert_eq!(config.sections[0].id, "connections");
        assert_eq!(config.sections[0].item_ids, ["prod", "dev"]);
        assert_eq!(config.sections[1].id, "scheduled");
        assert_eq!(config.tunnels.len(), 2);

        let (_, second_snapshot) = Config::load_with_snapshot(&paths).unwrap();
        assert_eq!(second_snapshot, snapshot);
        assert_eq!(fs::read_to_string(&backup).unwrap(), legacy);

        fs::remove_dir_all(paths.directory).unwrap();
    }

    #[test]
    fn explicitly_versioned_v1_remains_migratable() {
        let value: toml::Value = r#"
version = 1
[commands.hello]
name = "Hello"
command = "echo"
args = ["hello"]
group_header = "TOOLS"
"#
        .parse()
        .unwrap();

        let migrated = migrate_v1_to_v2(value).unwrap();
        assert_eq!(migrated.version, CURRENT_CONFIG_VERSION);
        assert_eq!(migrated.sections[0].id, "tools");
        assert_eq!(
            migrated.sections[0].items[0].run.as_ref().unwrap()[0],
            "echo"
        );
    }

    #[test]
    fn rejects_unknown_future_versions_without_rewriting() {
        let paths = test_paths("future-version");
        let future = "version = 99\n";
        fs::write(paths.config_path(), future).unwrap();

        let error = Config::load_with_snapshot(&paths).unwrap_err().to_string();
        assert!(error.contains("Unsupported config version 99"));
        assert_eq!(fs::read_to_string(paths.config_path()).unwrap(), future);
        assert!(!paths.directory.join("config.toml.v99.bak").exists());

        fs::remove_dir_all(paths.directory).unwrap();
    }
}
