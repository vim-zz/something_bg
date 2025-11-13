// src/main.rs
//
// The main entry point. We keep macOS-specific setup code here (NSApplication, run loop).
// We also define the global reference `GLOBAL_APP` so that the toggleTunnel function can
// look up the instance of `App` easily. Alternatively, you can store the `App` reference
// inside the Objective-C handler class.

use log::info;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSNotificationCenter};
use std::sync::OnceLock;

mod app;
mod config;
mod logger;
mod menu;
mod scheduler;
mod tunnel;

use app::App;

// Expose the global App so that `toggleTunnel` can access it.
// This is just an example—there are alternative approaches for bridging
// global state to an Objective-C selector.
pub static GLOBAL_APP: OnceLock<app::App> = OnceLock::new();

pub fn application_will_terminate_handler() {
    info!("Application is terminating; cleaning up tunnels...");
    if let Some(app) = GLOBAL_APP.get() {
        app.cleanup_tunnels();
    }
}

/// The main function: sets up Cocoa, the app, logger, menu, etc.
fn main() {
    // 1. Initialize the logger
    logger::init_logger();
    info!("Application starting up");

    // 2. Get the main thread marker (required for AppKit APIs)
    let mtm = MainThreadMarker::new().expect("Must be on main thread");

    // 3. Cocoa setup
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // 4. Create the handler (Objective-C class) for menu events
    let handler = menu::MenuHandler::new(mtm);

    // 5. Create the status bar item with attached menu
    let status_item = menu::create_status_item(&handler, mtm);

    // Store the app in the global variable
    let mut the_app = App::new();
    the_app.set_status_item(status_item);
    GLOBAL_APP.set(the_app).ok().unwrap();

    // 6. Observe application termination
    let notification_center = NSNotificationCenter::defaultCenter();
    unsafe {
        let notification_name =
            objc2_foundation::NSString::from_str("NSApplicationWillTerminateNotification");
        notification_center.addObserver_selector_name_object(
            &handler,
            objc2::sel!(applicationWillTerminate:),
            Some(&notification_name),
            None,
        );
    }

    // 7. Run the main application loop
    app.run();
}
