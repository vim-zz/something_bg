// src/menu.rs
//
// Responsible for creating the NSStatusItem and NSMenu, plus the Objective-C class
// that receives menu events. We keep the function references the same, but route
// the logic to `toggleTunnel` in `tunnel.rs`.

use log::{error, warn};
use objc2::{ClassType, MainThreadOnly, define_class, rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString, ns_string};

use crate::config::{Config, TunnelConfig};

// These are backup icons if image loading fails
const ICON_INACTIVE: &str = "○"; // Empty circle for idle
const ICON_ACTIVE: &str = "●"; // Filled circle for active

fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// Declare the MenuHandler class using objc2's define_class! macro
define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - MenuHandler does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MenuHandler"]
    pub struct MenuHandler;

    unsafe impl NSObjectProtocol for MenuHandler {}

    impl MenuHandler {
        #[unsafe(method(toggleTunnel:))]
        fn toggle_tunnel(&self, item: &NSMenuItem) {
            crate::tunnel::toggle_tunnel_handler(item);
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: &NSObject) {
            crate::application_will_terminate_handler();
        }
    }
);

impl MenuHandler {
    pub fn new(_mtm: MainThreadMarker) -> Retained<Self> {
        let cls = Self::class();
        unsafe {
            let obj: Retained<Self> = objc2::msg_send![cls, new];
            obj
        }
    }
}

/// Create the NSMenu for the status item.
pub fn create_menu(handler: &MenuHandler, mtm: MainThreadMarker) -> Retained<NSMenu> {
    unsafe {
        let menu = NSMenu::new(mtm);

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
            // Add group header if specified
            if let Some(group_header) = &tunnel_config.group_header {
                let header_item = create_header_item(group_header, tunnel_config.group_icon.as_deref(), mtm);
                menu.addItem(&header_item);
            }

            let menu_item = create_menu_item(handler, tunnel_config, key, mtm);
            menu.addItem(&menu_item);

            // Add separator after this item if configured
            if tunnel_config.separator_after.unwrap_or(false) {
                let separator = NSMenuItem::separatorItem(mtm);
                menu.addItem(&separator);
            }
        }

        // Add Separator before About
        let separator1 = NSMenuItem::separatorItem(mtm);
        menu.addItem(&separator1);

        // Add About item
        let about_title = NSString::from_str(&format!(
            "Something in the Background (v{})",
            get_app_version()
        ));
        let about_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &about_title,
            Some(sel!(orderFrontStandardAboutPanel:)),
            ns_string!(""),
        );
        about_item.setTarget(Some(handler as &AnyObject));
        menu.addItem(&about_item);

        // Add Separator before Quit
        let separator2 = NSMenuItem::separatorItem(mtm);
        menu.addItem(&separator2);

        // Quit menu item
        let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            ns_string!("Quit"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        );

        menu.addItem(&quit_item);

        menu
    }
}

/// Helper to create a header menu item (non-clickable section title)
fn create_header_item(title: &str, icon_spec: Option<&str>, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    unsafe {
        let title_ns = NSString::from_str(title);
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &title_ns,
            None, // No action - non-clickable
            ns_string!(""),
        );

        // Make it disabled (non-clickable) and use as section header
        item.setEnabled(false);

        // Load and set icon if specified
        if let Some(icon) = icon_spec {
            if let Some(image) = load_icon(icon) {
                item.setImage(Some(&image));
            }
        }

        item
    }
}

/// Helper to create a single NSMenuItem for toggling a tunnel.
fn create_menu_item(
    handler: &MenuHandler,
    tunnel_config: &TunnelConfig,
    command_id: &str,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    unsafe {
        let title_ns = NSString::from_str(&tunnel_config.name);
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &title_ns,
            Some(sel!(toggleTunnel:)),
            ns_string!(""),
        );

        let command_id_ns = NSString::from_str(command_id);
        item.setRepresentedObject(Some(&command_id_ns));
        item.setTarget(Some(handler as &AnyObject));
        item.setState(0); // NSOffState = 0

        item
    }
}

/// Load an icon from an SF Symbol.
/// SF Symbol format: "sf:symbol.name"
fn load_icon(icon_spec: &str) -> Option<Retained<NSImage>> {
    if icon_spec.starts_with("sf:") {
        // Load SF Symbol (macOS 11+)
        let symbol_name = &icon_spec[3..];
        let symbol_ns = NSString::from_str(symbol_name);

        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&symbol_ns, None);
        if let Some(img) = image {
            // Set image size to 16x16 for menu items
            img.setSize(objc2_foundation::NSSize { width: 16.0, height: 16.0 });
            Some(img)
        } else {
            warn!("Failed to load SF Symbol: {}", symbol_name);
            None
        }
    } else {
        warn!("Unsupported icon format: {}. Use 'sf:symbol.name'", icon_spec);
        None
    }
}

/// Creates a status bar item and attaches the menu to it.
pub fn create_status_item(handler: &MenuHandler, mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(-1.0);

    if let Some(button) = status_item.button(mtm) {
        let title = NSString::from_str(ICON_INACTIVE);
        button.setTitle(&title);
    }

    status_item.setMenu(Some(&create_menu(handler, mtm)));
    status_item
}

pub fn update_status_item_title(status_item: &NSStatusItem, active: bool, mtm: MainThreadMarker) {
    if let Some(button) = status_item.button(mtm) {
        let title_str = if active { ICON_ACTIVE } else { ICON_INACTIVE };
        let title = NSString::from_str(title_str);
        button.setTitle(&title);
    }
}
