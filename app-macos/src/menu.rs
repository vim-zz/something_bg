// src/menu.rs
//
// Responsible for creating the NSStatusItem and NSMenu, plus the Objective-C class
// that receives menu events. We keep the function references the same, but route
// the logic to `toggleTunnel` in `tunnel.rs`.

use log::{error, warn};
use objc2::{
    ClassType, MainThreadOnly, define_class, rc::Retained, runtime::AnyObject,
    runtime::ProtocolObject, sel,
};
use objc2_app_kit::{NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString, ns_string};

use crate::GLOBAL_APP;
use something_bg_core::config::{Config, ScheduledTaskConfig, TunnelConfig};

// These are backup icons if image loading fails
const ICON_INACTIVE: &str = "○"; // Empty circle for idle
const ICON_ACTIVE: &str = "●"; // Filled circle for active

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

    unsafe impl NSMenuDelegate for MenuHandler {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            update_scheduled_task_items(menu);
        }
    }

    impl MenuHandler {
        #[unsafe(method(toggleTunnel:))]
        fn toggle_tunnel(&self, item: &NSMenuItem) {
            toggle_tunnel_handler(item);
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: &NSObject) {
            crate::application_will_terminate_handler();
        }

        #[unsafe(method(openConfigFolder:))]
        fn open_config_folder(&self, _item: &NSMenuItem) {
            something_bg_core::config::open_config_folder_handler();
        }

        #[unsafe(method(runScheduledTask:))]
        fn run_scheduled_task(&self, item: &NSMenuItem) {
            run_scheduled_task_handler(item);
        }

        #[unsafe(method(displayAppInfo:))]
        fn display_app_info(&self, _item: &NSMenuItem) {
            crate::about::show_about_window();
        }

        #[unsafe(method(openGitHubURL:))]
        fn open_github_url(&self, _sender: &AnyObject) {
            crate::about::open_github_url();
        }

        #[unsafe(method(exitApplication:))]
        fn exit_application(&self, _item: &NSMenuItem) {
            exit_application_handler();
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

/// Handler function for exiting the application
fn exit_application_handler() {
    use log::info;

    info!("Exiting application");

    if let Some(mtm) = MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        unsafe {
            let _: () = objc2::msg_send![&app, terminate: std::ptr::null::<AnyObject>()];
        }
    }
}

/// Handler function for manually running a scheduled task
fn run_scheduled_task_handler(item: &NSMenuItem) {
    use log::info;

    if let Some(represented_obj) = item.representedObject() {
        // SAFETY: We know we stored an NSString as the represented object
        let task_id_str = extract_nsstring_from_object(&represented_obj);

        info!("Manually triggering scheduled task: {}", task_id_str);

        if let Some(app) = crate::GLOBAL_APP.get() {
            if let Err(e) = app.task_scheduler.run_task_now(&task_id_str) {
                error!("Failed to run task '{}': {}", task_id_str, e);
            }
            // Note: The menu will update automatically next time it's opened
            // via the menuNeedsUpdate delegate method
        }
    }
}

/// Safely extracts an NSString from a represented object
/// SAFETY: Caller must ensure the object is actually an NSString
fn extract_nsstring_from_object(obj: &AnyObject) -> String {
    // Use objc2's safe casting mechanism
    let ns_string: &NSString = unsafe { &*(obj as *const AnyObject as *const NSString) };
    ns_string.to_string()
}

/// Helper to create an NSMenuItem with action
/// Wraps the unsafe initWithTitle_action_keyEquivalent call
fn create_menu_item_with_action(
    title: &NSString,
    action: Option<objc2::runtime::Sel>,
    key_equivalent: &NSString,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: This is a standard Cocoa pattern for creating menu items
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), title, action, key_equivalent)
    }
}

/// Helper to set represented object on a menu item
/// Wraps the unsafe setRepresentedObject call
fn set_menu_item_represented_object(item: &NSMenuItem, obj: &NSString) {
    // SAFETY: We're storing a valid NSString object
    unsafe { item.setRepresentedObject(Some(obj)) };
}

/// Helper to set target on a menu item
/// Wraps the unsafe setTarget call
fn set_menu_item_target(item: &NSMenuItem, target: &AnyObject) {
    // SAFETY: target must be a valid object that responds to the item's action selector
    unsafe { item.setTarget(Some(target)) };
}

/// Handle toggling a tunnel menu item by delegating into the shared App state.
fn toggle_tunnel_handler(item: &NSMenuItem) {
    // Identify if the menu item is currently active or not.
    let state = item.state();
    let new_state = if state == 1 { 0 } else { 1 }; // NSOnState = 1, NSOffState = 0
    item.setState(new_state);

    // Extract the command key from the menu item
    if let Some(command_id) = item.representedObject() {
        let command_key = extract_nsstring_from_object(&command_id);

        if let Some(app) = GLOBAL_APP.get() {
            let enable = new_state == 1;
            let any_active = app.tunnel_manager.toggle(&command_key, enable);

            // Update the status item icon if we have a reference to it
            if let Some(status_item) = app.get_status_item() {
                if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
                    update_status_item_title(&status_item, any_active, mtm);
                }
            }
        }
    }
}

/// Update scheduled task items in the menu to show current "Last run" times
fn update_scheduled_task_items(menu: &NSMenu) {
    use something_bg_core::scheduler::format_last_run;

    // Get the app to access the scheduler
    let Some(app) = crate::GLOBAL_APP.get() else {
        return;
    };

    // Iterate through menu items to find scheduled tasks (items with submenus)
    let num_items = menu.numberOfItems();
    for i in 0..num_items {
        if let Some(item) = menu.itemAtIndex(i) {
            // Check if this item has a submenu (scheduled tasks have submenus)
            if let Some(submenu) = item.submenu() {
                // The submenu should have items in this order:
                // 0: Schedule: ...
                // 1: Next run: ...
                // 2: Last run: ...
                // 3: Separator
                // 4: Run Now

                if submenu.numberOfItems() >= 3 {
                    // Try to get the task ID from the "Run Now" item (index 4)
                    if let Some(run_now_item) = submenu.itemAtIndex(4) {
                        if let Some(represented_obj) = run_now_item.representedObject() {
                            let task_id_str = extract_nsstring_from_object(&represented_obj);

                            // Get updated task info from scheduler
                            if let Some(task) = app.task_scheduler.get_task(&task_id_str) {
                                // Update "Next run" item (index 1)
                                if let Some(next_run_item) = submenu.itemAtIndex(1) {
                                    let next_run_text = format_last_run(&task.next_run);
                                    let new_title =
                                        NSString::from_str(&format!("Next run: {}", next_run_text));
                                    next_run_item.setTitle(&new_title);
                                }

                                // Update "Last run" item (index 2)
                                if let Some(last_run_item) = submenu.itemAtIndex(2) {
                                    let last_run_text = format_last_run(&task.last_run);
                                    let new_title =
                                        NSString::from_str(&format!("Last run: {}", last_run_text));
                                    last_run_item.setTitle(&new_title);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Create the NSMenu for the status item.
pub fn create_menu(handler: &MenuHandler, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);

    // Set the delegate so menuNeedsUpdate gets called
    let delegate = ProtocolObject::from_ref(handler);
    menu.setDelegate(Some(delegate));

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
            let header_item =
                create_header_item(group_header, tunnel_config.group_icon.as_deref(), mtm);
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

    // Add scheduled tasks section
    if !config.schedules.is_empty() {
        let separator = NSMenuItem::separatorItem(mtm);
        menu.addItem(&separator);

        for (key, task_config) in config.schedules.iter() {
            // Add group header if specified
            if let Some(group_header) = &task_config.group_header {
                let header_item =
                    create_header_item(group_header, task_config.group_icon.as_deref(), mtm);
                menu.addItem(&header_item);
            }

            let scheduled_menu_item = create_scheduled_task_item(handler, task_config, key, mtm);
            menu.addItem(&scheduled_menu_item);

            // Add separator after this item if configured
            if task_config.separator_after.unwrap_or(false) {
                let separator = NSMenuItem::separatorItem(mtm);
                menu.addItem(&separator);
            }
        }
    }

    // Add Separator before Open Config Folder
    let separator1 = NSMenuItem::separatorItem(mtm);
    menu.addItem(&separator1);

    // Add "Open Config Folder" item
    let config_folder_item = create_menu_item_with_action(
        ns_string!("Open Config Folder"),
        Some(sel!(openConfigFolder:)),
        ns_string!(""),
        mtm,
    );
    set_menu_item_target(&config_folder_item, handler as &AnyObject);
    menu.addItem(&config_folder_item);

    // Add About item (clickable, opens About window)
    let about_item = create_menu_item_with_action(
        ns_string!("About"),
        Some(sel!(displayAppInfo:)),
        ns_string!(""),
        mtm,
    );
    set_menu_item_target(&about_item, handler as &AnyObject);
    menu.addItem(&about_item);

    // Add Separator before Quit
    let separator1 = NSMenuItem::separatorItem(mtm);
    menu.addItem(&separator1);

    // Quit menu item (using custom selector to avoid automatic symbol)
    let quit_item = create_menu_item_with_action(
        ns_string!("Quit Something in the Background"),
        Some(sel!(exitApplication:)),
        ns_string!("q"),
        mtm,
    );
    set_menu_item_target(&quit_item, handler as &AnyObject);
    menu.addItem(&quit_item);

    menu
}

/// Helper to create a header menu item (non-clickable section title)
fn create_header_item(
    title: &str,
    icon_spec: Option<&str>,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let title_ns = NSString::from_str(title);
    let item = create_menu_item_with_action(&title_ns, None, ns_string!(""), mtm);

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

/// Helper to create a single NSMenuItem for toggling a tunnel.
fn create_menu_item(
    handler: &MenuHandler,
    tunnel_config: &TunnelConfig,
    command_id: &str,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let title_ns = NSString::from_str(&tunnel_config.name);
    let item =
        create_menu_item_with_action(&title_ns, Some(sel!(toggleTunnel:)), ns_string!(""), mtm);

    let command_id_ns = NSString::from_str(command_id);
    set_menu_item_represented_object(&item, &command_id_ns);
    set_menu_item_target(&item, handler as &AnyObject);
    item.setState(0); // NSOffState = 0

    item
}

/// Helper to create a menu item for a scheduled task with submenu
fn create_scheduled_task_item(
    handler: &MenuHandler,
    task_config: &ScheduledTaskConfig,
    task_id: &str,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // Main menu item with task name
    let title_ns = NSString::from_str(&task_config.name);
    let item = create_menu_item_with_action(&title_ns, None, ns_string!(""), mtm);

    // Create submenu
    let submenu = NSMenu::new(mtm);

    // Get task info from scheduler if available
    let (schedule_text, last_run_text) = if let Some(app) = crate::GLOBAL_APP.get() {
        let schedule = if let Some(task) = app.task_scheduler.get_task(task_id) {
            something_bg_core::scheduler::cron_to_human_readable(&task.cron_schedule)
        } else {
            something_bg_core::scheduler::cron_to_human_readable(&task_config.cron_schedule)
        };

        let last_run = if let Some(task) = app.task_scheduler.get_task(task_id) {
            something_bg_core::scheduler::format_last_run(&task.last_run)
        } else {
            "Never".to_string()
        };

        (schedule, last_run)
    } else {
        (
            something_bg_core::scheduler::cron_to_human_readable(&task_config.cron_schedule),
            "Never".to_string(),
        )
    };

    // Add schedule info (disabled/grayed out)
    let schedule_title = NSString::from_str(&format!("Schedule: {}", schedule_text));
    let schedule_item = create_menu_item_with_action(&schedule_title, None, ns_string!(""), mtm);
    schedule_item.setEnabled(false);
    submenu.addItem(&schedule_item);

    // Add next run info (disabled/grayed out)
    let next_run_text = if let Some(app) = crate::GLOBAL_APP.get() {
        if let Some(task) = app.task_scheduler.get_task(task_id) {
            something_bg_core::scheduler::format_last_run(&task.next_run)
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    };
    let next_run_title = NSString::from_str(&format!("Next run: {}", next_run_text));
    let next_run_item = create_menu_item_with_action(&next_run_title, None, ns_string!(""), mtm);
    next_run_item.setEnabled(false);
    submenu.addItem(&next_run_item);

    // Add last run info (disabled/grayed out)
    let last_run_title = NSString::from_str(&format!("Last run: {}", last_run_text));
    let last_run_item = create_menu_item_with_action(&last_run_title, None, ns_string!(""), mtm);
    last_run_item.setEnabled(false);
    submenu.addItem(&last_run_item);

    // Add separator
    let separator = NSMenuItem::separatorItem(mtm);
    submenu.addItem(&separator);

    // Add "Run Now" action
    let run_now_item = create_menu_item_with_action(
        ns_string!("Run Now"),
        Some(sel!(runScheduledTask:)),
        ns_string!(""),
        mtm,
    );
    let task_id_ns = NSString::from_str(task_id);
    set_menu_item_represented_object(&run_now_item, &task_id_ns);
    set_menu_item_target(&run_now_item, handler as &AnyObject);
    submenu.addItem(&run_now_item);

    // Attach submenu to main item
    item.setSubmenu(Some(&submenu));

    item
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
            img.setSize(objc2_foundation::NSSize {
                width: 16.0,
                height: 16.0,
            });
            Some(img)
        } else {
            warn!("Failed to load SF Symbol: {}", symbol_name);
            None
        }
    } else {
        warn!(
            "Unsupported icon format: {}. Use 'sf:symbol.name'",
            icon_spec
        );
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
