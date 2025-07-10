// src/menu.rs
//
// Responsible for creating the NSStatusItem and NSMenu, plus the Objective-C class
// that receives menu events. We keep the function references the same, but route
// the logic to `toggleTunnel` in `tunnel.rs`.

use cocoa::appkit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use cocoa::base::{id, nil, NO, YES, BOOL};
use cocoa::foundation::{NSAutoreleasePool, NSString, NSSize};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::path::PathBuf;
use log::{error, warn};

use crate::{applicationWillTerminate, tunnel::toggleTunnel};
use crate::config::Config;

// These are backup icons if image loading fails
const ICON_INACTIVE: &str = "○";  // Empty circle for idle
const ICON_ACTIVE: &str = "●";   // Filled circle for active

fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Registers our Objective-C class, `MenuHandler`, with the selectors
/// for toggling tunnels and handling app termination.
pub fn register_selector() -> *const Class {
    unsafe {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("MenuHandler", superclass).unwrap();

        // Link the "toggleTunnel:" selector to our Rust function
        decl.add_method(
            sel!(toggleTunnel:),
            toggleTunnel as extern "C" fn(&Object, Sel, id),
        );

        // Link the "applicationWillTerminate:" selector
        decl.add_method(
            sel!(applicationWillTerminate:),
            applicationWillTerminate as extern "C" fn(&Object, Sel, id),
        );

        decl.register()
    }
}

/// Create the NSMenu for the status item.
pub fn create_menu(handler: id) -> id {
    unsafe {
        let menu = NSMenu::new(nil).autorelease();

        // Load configuration and create menu items dynamically
        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                error!("Failed to load configuration for menu: {}", e);
                warn!("Using default configuration for menu");
                Config::default()
            }
        };

        // Create menu items from configuration
        for (key, tunnel_config) in config.tunnels.iter() {
            let menu_item = create_menu_item(handler, &tunnel_config.name, key);
            menu.addItem_(menu_item);
        }

        // Add Separator before About
        let separator1 = NSMenuItem::separatorItem(nil);
        menu.addItem_(separator1);

        // Add About item
        let about_title =
            NSString::alloc(nil).init_str(&format!("Something in the Background (v{})", get_app_version()));
        let about_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
            about_title,
            sel!(orderFrontStandardAboutPanel:),
            NSString::alloc(nil).init_str(""),
        );
        about_item.setTarget_(handler);
        menu.addItem_(about_item);

        // Add Separator before About
        let separator1 = NSMenuItem::separatorItem(nil);
        menu.addItem_(separator1);

        // Quit menu item
        let quit_title = NSString::alloc(nil).init_str("Quit");
        let quit_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
            quit_title,
            sel!(terminate:),
            NSString::alloc(nil).init_str("q"),
        );

        menu.addItem_(quit_item);

        menu
    }
}

/// Helper to create a single NSMenuItem for toggling a tunnel.
fn create_menu_item(handler: id, title: &str, command_id: &str) -> id {
    unsafe {
        let title_ns = NSString::alloc(nil).init_str(title);
        let item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
            title_ns,
            sel!(toggleTunnel:),
            NSString::alloc(nil).init_str(""),
        );

        let command_id_ns = NSString::alloc(nil).init_str(command_id);
        let _: () = msg_send![item, setRepresentedObject: command_id_ns];
        let _: () = msg_send![item, setTarget: handler];
        let _: () = msg_send![item, setState: NO];

        item
    }
}

/// Creates a status bar item and attaches the menu to it.
pub fn create_status_item(handler: id) -> id {
    unsafe {
        let status_bar = NSStatusBar::systemStatusBar(nil);
        let status_item = status_bar.statusItemWithLength_(-1.0);

        let button: id = msg_send![status_item, button];

        // Try to load the image first
        let image = load_status_bar_image(false); // Start with inactive state

        if image != nil {
            // If image loaded successfully, use it
            let _: () = msg_send![button, setImage: image];
        } else {
            // Fall back to text if image loading fails
            let title = NSString::alloc(nil).init_str(ICON_INACTIVE);
            let _: () = msg_send![button, setTitle: title];
        }

        status_item.setMenu_(create_menu(handler));
        status_item
    }
}

fn load_status_bar_image(active: bool) -> id {
    return nil;

    unsafe {
        let image: id = msg_send![class!(NSImage), new];
        let size = NSSize::new(16.0, 16.0);
        let _: () = msg_send![image, setSize:size];

        // Path to the image based on state (active or inactive)
        let image_name = if active { "peacock_open_16x16.png" } else { "peacock_folded_16x16.png" };

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("images")
            .join(image_name);

        if path.exists() {
            let path_str = path.to_str().unwrap_or("");
            let path_ns = NSString::alloc(nil).init_str(path_str);

            let _: () = msg_send![image, initWithContentsOfFile:path_ns];
            let _: () = msg_send![image, setTemplate:YES as BOOL];
            return image;
        }

        // If image loading fails, return nil
        nil
    }
}

pub fn update_status_item_title(status_item: id, active: bool) {
    unsafe {
        let button: id = msg_send![status_item, button];

        // Try to load the image first
        let image = load_status_bar_image(active);

        if image != nil {
            // If image loaded successfully, use it
            let _: () = msg_send![button, setImage: image];
            let _: () = msg_send![button, setTitle: nil]; // Clear any existing title
        } else {
            // Fall back to text if image loading fails
            let title = NSString::alloc(nil).init_str(if active { ICON_ACTIVE } else { ICON_INACTIVE });
            let _: () = msg_send![button, setTitle: title];
        }
    }
}
