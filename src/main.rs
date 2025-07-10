// src/main.rs
//
// The main entry point. We keep macOS-specific setup code here (NSApplication, run loop).
// We also define the global reference `GLOBAL_APP` so that the toggleTunnel function can
// look up the instance of `App` easily. Alternatively, you can store the `App` reference
// inside the Objective-C handler class.

use cocoa::appkit::{NSApplication, NSApplicationActivationPolicy};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSAutoreleasePool, NSString};
use log::info;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::OnceLock;

mod app;
mod config;
mod logger;
mod menu;
mod tunnel;

use app::App;

// Expose the global App so that `toggleTunnel` can access it.
// This is just an example—there are alternative approaches for bridging
// global state to an Objective-C selector.
pub static GLOBAL_APP: OnceLock<app::App> = OnceLock::new();

#[unsafe(no_mangle)]
extern "C" fn applicationWillTerminate(_: &Object, _: Sel, _notification: id) {
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

    unsafe {
        // 3. Cocoa setup
        let _pool = NSAutoreleasePool::new(nil);
        let app = NSApplication::sharedApplication(nil);
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
        );

        // 4. Create the handler (Objective-C class) for menu events
        let handler_class = menu::register_selector();
        let handler: id = msg_send![handler_class, new];

        // 5. Create the status bar item with attached menu
        let status_item = menu::create_status_item(handler);
        // Store the app in the global variable
        let mut the_app = App::new();
        the_app.set_status_item(status_item);
        GLOBAL_APP.set(the_app).ok().unwrap();

        // 6. Observe application termination
        let notification_center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
        let _: () = msg_send![notification_center,
            addObserver: handler
            selector: sel!(applicationWillTerminate:)
            name: NSString::alloc(nil).init_str("NSApplicationWillTerminateNotification")
            object: nil
        ];

        // 7. Run the main application loop
        app.run();
    }
}
