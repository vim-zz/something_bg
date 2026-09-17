// src/app.rs
//
// Defines the `App` structure holding shared state (commands, active tunnels).
// Also provides methods for cleanup or other global operations.

use log::{error, info, warn};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{ClassType, MainThreadOnly, define_class};
use objc2_app_kit::NSStatusItem;
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use something_bg_core::command::{CommandRunner, format_duration as format_elapsed};
use something_bg_core::config::{Config, ConfigMonitor, LoginSettingUpdate};
use something_bg_core::platform::AppPaths;
use something_bg_core::scheduler::TaskScheduler;
use something_bg_core::tunnel::{TunnelFailure, TunnelManager};

use crate::paths::MacPaths;

// Wrapper type to make the status item thread-safe
pub struct StatusItemWrapper(pub Retained<NSStatusItem>);
unsafe impl Send for StatusItemWrapper {}
unsafe impl Sync for StatusItemWrapper {}

/// Primary application structure. Contains references to any data that
/// must be shared across modules (e.g., commands, active tunnels).
pub struct App {
    pub tunnel_manager: TunnelManager,
    pub tunnel_names: Mutex<HashMap<String, String>>,
    pub command_runner: Mutex<CommandRunner>,
    pub task_scheduler: TaskScheduler,
    pub paths: Arc<MacPaths>,
    pub status_item: Option<Arc<Mutex<StatusItemWrapper>>>,
    config_monitor: ConfigMonitor,
    start_at_login: Mutex<Option<bool>>,
}

impl App {
    /// Creates a new `App` with commands loaded from config file.
    pub fn new() -> (Self, Config) {
        let paths = Arc::new(MacPaths::default());

        // Load configuration from TOML file
        let (config, config_contents) = match Config::load_with_snapshot(paths.as_ref()) {
            Ok((config, contents)) => {
                info!("Loaded configuration successfully");
                (config, Some(contents))
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                warn!("Using default configuration");
                (Config::default(), None)
            }
        };

        let commands = config.to_tunnel_commands();
        let path = config.get_path();

        // Initialize the tunnel manager
        let tunnel_manager = TunnelManager::new(commands, config.get_path());

        // Initialize the command runner
        let mut command_runner = CommandRunner::new(config.get_path());
        let history_log = paths
            .config_path()
            .parent()
            .unwrap()
            .join("command_history.log");
        command_runner.set_history_path(history_log);

        // Set macOS notify callback using native NSUserNotificationCenter
        // (shows the app icon instead of Script Editor)
        command_runner.set_notify_callback(std::sync::Arc::new(|event| {
            if event.is_running {
                send_notification(event.name, "\u{23f3} Running...");
                return;
            }
            let time_str = match event.elapsed {
                Some(d) if d.as_secs() >= 2 => format!(" ({})", format_elapsed(d)),
                _ => String::new(),
            };
            let status = if event.success {
                "\u{2705} Completed"
            } else {
                "\u{274c} Failed"
            };
            let body = if event.output.is_empty() {
                format!("{}{}", status, time_str)
            } else {
                format!("{}{}\n{}", status, time_str, event.output)
            };
            send_notification(event.name, &body);
        }));

        // Set macOS terminal callback using osascript
        command_runner.set_terminal_callback(std::sync::Arc::new(|command, args| {
            let full_cmd = if args.is_empty() {
                command.to_string()
            } else {
                format!(
                    "{} {}",
                    command,
                    args.iter()
                        .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            let script = format!(
                "tell application \"Terminal\" to do script \"{}\"",
                full_cmd.replace('\\', "\\\\").replace('"', "\\\"")
            );
            if let Err(e) = std::process::Command::new("osascript")
                .args(["-e", &script])
                .spawn()
            {
                log::warn!("Failed to open Terminal: {}", e);
            }
        }));

        // Register commands from config
        command_runner.register_all(&config.commands);

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

        let app = Self {
            tunnel_manager,
            tunnel_names: Mutex::new(
                config
                    .tunnels
                    .iter()
                    .map(|(key, tunnel)| (key.clone(), tunnel.name.clone()))
                    .collect(),
            ),
            command_runner: Mutex::new(command_runner),
            task_scheduler,
            paths: paths.clone(),
            status_item: None,
            start_at_login: Mutex::new(
                config_contents
                    .as_ref()
                    .map(|_| config.settings.start_at_login),
            ),
            config_monitor: ConfigMonitor::new(paths.config_path(), config_contents),
        };

        (app, config)
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

    pub fn config_changed(&self) -> bool {
        self.config_monitor.has_changed().unwrap_or_else(|e| {
            warn!("Failed to check config for changes: {e}");
            false
        })
    }

    pub fn apply_login_preference(&self) -> Result<(), String> {
        // A malformed config must not change the user's existing OS preference.
        if let Some(enabled) = *self.start_at_login.lock().unwrap() {
            let status = crate::login_item::apply_preference(enabled)?;
            if status == crate::login_item::Status::RequiresApproval {
                send_login_notification(
                    "Allow Something in the Background in System Settings > General > Login Items. You can open it from the app’s Settings menu.",
                );
            }
        }
        Ok(())
    }

    pub fn toggle_start_at_login(&self) -> Result<(), String> {
        let previous = crate::login_item::status()?;
        let enabled = !previous.requested();
        let update = LoginSettingUpdate::prepare(self.paths.as_ref(), enabled)
            .map_err(|error| error.to_string())?;
        crate::login_item::set_enabled(enabled)?;
        if let Err(error) = update.save() {
            // Restore the OS preference if the config could not be saved.
            return match crate::login_item::set_enabled(previous.requested()) {
                Ok(_) => Err(format!("Could not save Start at Login: {error}")),
                Err(rollback) => Err(format!(
                    "Could not save Start at Login: {error}. Restoring the previous login setting also failed: {rollback}"
                )),
            };
        }
        *self.start_at_login.lock().unwrap() = Some(enabled);
        self.config_monitor
            .mark_setting_applied(&update.original, update.updated);
        Ok(())
    }

    pub fn reload_config(&self) -> Result<Config, String> {
        let (config, contents) =
            Config::load_with_snapshot(self.paths.as_ref()).map_err(|e| e.to_string())?;
        let path = config.get_path();

        // Fail before replacing runtime commands/menu data if macOS rejects the preference.
        crate::login_item::apply_preference(config.settings.start_at_login)?;
        self.task_scheduler
            .reconfigure(path.clone(), &config.schedules)?;
        self.tunnel_manager
            .reconfigure(config.to_tunnel_commands(), path.clone());
        self.command_runner
            .lock()
            .unwrap()
            .reconfigure(path, &config.commands);
        *self.tunnel_names.lock().unwrap() = config
            .tunnels
            .iter()
            .map(|(key, tunnel)| (key.clone(), tunnel.name.clone()))
            .collect();
        *self.start_at_login.lock().unwrap() = Some(config.settings.start_at_login);
        self.config_monitor.mark_applied(contents);

        info!("Reloaded configuration successfully");
        Ok(config)
    }
}

// Notification delegate: handles "Show" button clicks on notifications
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NotifDelegate"]
    pub struct NotifDelegate;

    unsafe impl NSObjectProtocol for NotifDelegate {}

    impl NotifDelegate {
        #[unsafe(method(userNotificationCenter:didActivateNotification:))]
        fn did_activate(&self, _center: &AnyObject, notification: &AnyObject) {
            // The notification carries its own snapshot so older notifications
            // still show the matching command, PATH, and logs after a retry or reload.
            let details: Option<Retained<objc2_foundation::NSDictionary<NSString, NSString>>> =
                unsafe { objc2::msg_send![notification, userInfo] };
            if details.as_ref().is_some_and(|info| info.objectForKey(objc2_foundation::ns_string!("loginItems")).is_some()) {
                if let Err(error) = crate::login_item::open_settings() {
                    warn!("Could not open Login Items settings: {error}");
                }
                return;
            }
            if let Some(details) = details
                && let (Some(name), Some(command), Some(logs)) = (
                    details.objectForKey(objc2_foundation::ns_string!("tunnelName")),
                    details.objectForKey(objc2_foundation::ns_string!("command")),
                    details.objectForKey(objc2_foundation::ns_string!("logs")),
                ) {
                let path = details.objectForKey(objc2_foundation::ns_string!("path"))
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "(not recorded for this notification)".to_owned());
                crate::connection_details::show(&name.to_string(), &command.to_string(), &path, &logs.to_string());
                return;
            }
            if let Some(app) = crate::GLOBAL_APP.get()
                && let Some(path) = app
                    .command_runner
                    .lock()
                    .unwrap()
                    .history_path()
                    .map(ToOwned::to_owned)
                && path.exists()
            {
                let _ = std::process::Command::new("open").arg(path).spawn();
            }
        }

        // Always show notifications even when the app is active (menu bar app)
        #[unsafe(method(userNotificationCenter:shouldPresentNotification:))]
        fn should_present(&self, _center: &AnyObject, _notification: &AnyObject) -> bool {
            true
        }
    }
);

impl NotifDelegate {
    pub fn new(_mtm: MainThreadMarker) -> Retained<Self> {
        let cls = Self::class();
        unsafe { objc2::msg_send![cls, new] }
    }
}

/// Set up native notification delivery with the app's icon and click-to-view-history.
/// Must be called on the main thread after GLOBAL_APP is set.
pub fn setup_notification_center(mtm: MainThreadMarker) {
    unsafe {
        let Some(center_class) = AnyClass::get(c"NSUserNotificationCenter") else {
            warn!("NSUserNotificationCenter not available");
            return;
        };
        let Some(center): Option<Retained<AnyObject>> =
            objc2::msg_send![center_class, defaultUserNotificationCenter]
        else {
            warn!("NSUserNotificationCenter unavailable for this process");
            return;
        };
        let delegate = NotifDelegate::new(mtm);
        let _: () = objc2::msg_send![&center, setDelegate: &*delegate];
        // Delegate must stay alive for the app lifetime; intentional leak for singleton
        std::mem::forget(delegate);
    }
    info!("Native notification center configured");
}

/// Send a native macOS notification using NSUserNotificationCenter.
/// Shows the app's icon and supports the "Show" action button.
pub fn send_notification(title: &str, body: &str) {
    deliver_notification(title, body, None, "View History");
}

pub fn send_login_notification(body: &str) {
    let details = objc2_foundation::NSDictionary::from_slices(
        &[objc2_foundation::ns_string!("loginItems")],
        &[objc2_foundation::ns_string!("true")],
    );
    deliver_notification("Start at Login", body, Some(&details), "Open Settings");
}

pub fn send_tunnel_failure(name: &str, failure: &TunnelFailure) {
    let name_ns = NSString::from_str(name);
    let command_ns = NSString::from_str(&failure.command_line);
    let path_ns = NSString::from_str(&failure.env_path);
    let logs_ns = NSString::from_str(&failure.logs());
    let details = objc2_foundation::NSDictionary::from_slices(
        &[
            objc2_foundation::ns_string!("tunnelName"),
            objc2_foundation::ns_string!("command"),
            objc2_foundation::ns_string!("path"),
            objc2_foundation::ns_string!("logs"),
        ],
        &[&*name_ns, &*command_ns, &*path_ns, &*logs_ns],
    );
    deliver_notification(
        &format!("{name} — Faulty"),
        "Connection failed. View Details to inspect and copy the command and error logs.",
        Some(&details),
        "View Details",
    );
}

fn deliver_notification(
    title: &str,
    body: &str,
    details: Option<&objc2_foundation::NSDictionary<NSString, NSString>>,
    action: &str,
) {
    unsafe {
        let Some(center_class) = AnyClass::get(c"NSUserNotificationCenter") else {
            warn!("NSUserNotificationCenter class not available");
            return;
        };
        let Some(center): Option<Retained<AnyObject>> =
            objc2::msg_send![center_class, defaultUserNotificationCenter]
        else {
            warn!("NSUserNotificationCenter unavailable for this process");
            return;
        };

        let Some(notif_class) = AnyClass::get(c"NSUserNotification") else {
            warn!("NSUserNotification class not available");
            return;
        };
        let notif: Retained<AnyObject> = objc2::msg_send![notif_class, new];

        let title_ns = NSString::from_str(title);
        let body_ns = NSString::from_str(body);
        let action_ns = NSString::from_str(action);
        if let Some(details) = details {
            let _: () = objc2::msg_send![&notif, setUserInfo: details];
        }
        let _: () = objc2::msg_send![&notif, setHasActionButton: true];

        let _: () = objc2::msg_send![&notif, setTitle: &*title_ns];
        let _: () = objc2::msg_send![&notif, setInformativeText: &*body_ns];
        let _: () = objc2::msg_send![&notif, setActionButtonTitle: &*action_ns];
        let _: () = objc2::msg_send![&center, deliverNotification: &*notif];
    }
}
