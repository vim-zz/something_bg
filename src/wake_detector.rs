// src/wake_detector.rs
//
// Detects when macOS wakes from sleep and triggers callbacks to check for missed scheduled tasks.
// Uses NSWorkspace notifications to observe system sleep/wake events.

use log::info;
use objc2::{ClassType, MainThreadOnly, define_class, rc::Retained};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use std::sync::{Arc, Mutex};

/// Callback type for wake notifications
type WakeCallback = Arc<Mutex<dyn Fn() + Send + 'static>>;

// Observer class for NSWorkspace wake notifications
define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - WakeObserver does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WakeObserver"]
    pub struct WakeObserver;

    unsafe impl NSObjectProtocol for WakeObserver {}

    impl WakeObserver {
        #[unsafe(method(workspaceDidWake:))]
        fn workspace_did_wake(&self, _notification: &NSNotification) {
            info!("macOS woke from sleep - checking for missed scheduled tasks");

            // Call the callback if it exists
            if let Some(callback) = get_wake_callback() {
                let cb = callback.lock().unwrap();
                (*cb)();
            }
        }
    }
);

impl WakeObserver {
    /// Create a new WakeObserver
    pub fn new() -> Retained<Self> {
        let cls = Self::class();
        unsafe {
            let obj: Retained<Self> = objc2::msg_send![cls, new];
            obj
        }
    }
}

/// Global storage for the wake callback
static WAKE_CALLBACK: Mutex<Option<WakeCallback>> = Mutex::new(None);

/// Set the callback to be called when the system wakes from sleep
pub fn set_wake_callback<F>(callback: F)
where
    F: Fn() + Send + 'static,
{
    let mut cb = WAKE_CALLBACK.lock().unwrap();
    *cb = Some(Arc::new(Mutex::new(callback)));
}

/// Get the current wake callback
fn get_wake_callback() -> Option<WakeCallback> {
    let cb = WAKE_CALLBACK.lock().unwrap();
    cb.clone()
}

/// Setup the wake observer to monitor system sleep/wake events
pub fn setup_wake_observer() -> Retained<WakeObserver> {
    info!("Setting up wake observer for system sleep/wake detection");

    let observer = WakeObserver::new();

    // Get the shared workspace and notification center
    let workspace = NSWorkspace::sharedWorkspace();
    let notification_center = workspace.notificationCenter();

    // Register for wake notifications
    unsafe {
        let notification_name =
            objc2_foundation::NSString::from_str("NSWorkspaceDidWakeNotification");
        notification_center.addObserver_selector_name_object(
            &observer,
            objc2::sel!(workspaceDidWake:),
            Some(&notification_name),
            None,
        );
    }

    info!("Wake observer registered successfully");
    observer
}
