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
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use something_bg_core::command::{CommandRunner, format_duration as format_elapsed};
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
    pub command_runner: CommandRunner,
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

        Self {
            tunnel_manager,
            command_runner,
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

// Notification delegate: handles "Show" button clicks on notifications
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NotifDelegate"]
    pub struct NotifDelegate;

    unsafe impl NSObjectProtocol for NotifDelegate {}

    impl NotifDelegate {
        #[unsafe(method(userNotificationCenter:didActivateNotification:))]
        fn did_activate(&self, _center: &AnyObject, _notification: &AnyObject) {
            if let Some(app) = crate::GLOBAL_APP.get()
                && let Some(path) = app.command_runner.history_path()
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
        let center: Retained<AnyObject> =
            objc2::msg_send![center_class, defaultUserNotificationCenter];
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
    unsafe {
        let Some(center_class) = AnyClass::get(c"NSUserNotificationCenter") else {
            warn!("NSUserNotificationCenter class not available");
            return;
        };
        let center: Retained<AnyObject> =
            objc2::msg_send![center_class, defaultUserNotificationCenter];

        let Some(notif_class) = AnyClass::get(c"NSUserNotification") else {
            warn!("NSUserNotification class not available");
            return;
        };
        let notif: Retained<AnyObject> = objc2::msg_send![notif_class, new];

        let title_ns = NSString::from_str(title);
        let body_ns = NSString::from_str(body);
        let action_ns = NSString::from_str("View History");

        let _: () = objc2::msg_send![&notif, setTitle: &*title_ns];
        let _: () = objc2::msg_send![&notif, setInformativeText: &*body_ns];
        let _: () = objc2::msg_send![&notif, setActionButtonTitle: &*action_ns];
        let _: () = objc2::msg_send![&center, deliverNotification: &*notif];
    }
}
