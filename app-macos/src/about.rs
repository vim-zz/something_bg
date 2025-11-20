// src/about.rs
//
// Handles the About window display and related functionality.

use log::{error, info};
use objc2::{ClassType, MainThreadOnly, define_class, rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSImage, NSImageScaling, NSImageView, NSTextField, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
    ns_string,
};
use std::cell::RefCell;

// Thread-local storage for the About window to prevent memory leaks.
// Using thread_local! because NSWindow and URLButtonHelper are main-thread-only objects.
thread_local! {
    static ABOUT_WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
    // Store URL helpers separately - they must outlive their windows to avoid use-after-free
    static URL_HELPERS: RefCell<Vec<Retained<URLButtonHelper>>> = const { RefCell::new(Vec::new()) };
}

/// Get the application version from Cargo.toml
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// Simple helper class for URL button click
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "URLButtonHelper"]
    struct URLButtonHelper;

    unsafe impl NSObjectProtocol for URLButtonHelper {}

    impl URLButtonHelper {
        #[unsafe(method(openURL:))]
        fn open_url(&self, _sender: &AnyObject) {
            open_github_url();
        }
    }
);

impl URLButtonHelper {
    fn new(_mtm: MainThreadMarker) -> Retained<Self> {
        let cls = Self::class();
        unsafe {
            let obj: Retained<Self> = objc2::msg_send![cls, new];
            obj
        }
    }
}

/// Opens the GitHub repository URL in the default browser
pub fn open_github_url() {
    use objc2_app_kit::NSWorkspace;

    info!("Opening GitHub URL");

    let url_string = NSString::from_str("https://github.com/vim-zz/something_bg");
    if let Some(url) = NSURL::URLWithString(&url_string) {
        let workspace = NSWorkspace::sharedWorkspace();
        workspace.openURL(&url);
    }
}

/// Handler function for showing the About window
pub fn show_about_window() {
    info!("Opening About window");

    let Some(mtm) = MainThreadMarker::new() else {
        error!("Failed to get MainThreadMarker for About window");
        return;
    };

    // Clear any previous window (this properly deallocates the old window)
    // We create a fresh window each time to avoid issues with closed windows
    ABOUT_WINDOW.with(|cell| {
        *cell.borrow_mut() = None;
    });

    // Create new window and URL helper
    let window = create_about_window(mtm);
    let url_helper = URLButtonHelper::new(mtm);

    // Setup the window content
    setup_window_content(&window, &url_helper, mtm);

    // Configure window behavior
    window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    window.makeKeyAndOrderFront(None);

    // Activate the application to ensure window is visible
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.activate();

    // Store URL helper separately - it must outlive the window because the button
    // holds a weak reference to it. We keep all helpers alive for the app lifetime.
    URL_HELPERS.with(|cell| {
        cell.borrow_mut().push(url_helper);
    });

    // Store window to prevent deallocation (proper lifecycle management)
    // This replaces std::mem::forget() - the old window is properly released,
    // and the new one is kept alive until the next call or app termination
    ABOUT_WINDOW.with(|cell| {
        *cell.borrow_mut() = Some(window);
    });
}

/// Creates the About window with proper frame and style
fn create_about_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(300.0, 280.0));
    let style_mask =
        NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame,
            style_mask,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    // Prevent AppKit from auto-releasing the window when closed
    // We manage the lifecycle ourselves via our Retained<> reference
    // SAFETY: We're taking ownership of the window's lifecycle management
    unsafe { window.setReleasedWhenClosed(false) };

    window.center();
    window
}

/// Sets up all the content views inside the About window
fn setup_window_content(window: &NSWindow, url_helper: &URLButtonHelper, mtm: MainThreadMarker) {
    let content_view = window.contentView().unwrap();

    // Add icon
    add_app_icon(&content_view, mtm);

    // Add app name label
    add_app_name_label(&content_view, mtm);

    // Add version label
    add_version_label(&content_view, mtm);

    // Add GitHub button
    add_github_button(&content_view, url_helper, mtm);
}

/// Adds the app icon (SF Symbol circle) to the window
fn add_app_icon(content_view: &objc2_app_kit::NSView, mtm: MainThreadMarker) {
    let image_view = NSImageView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(100.0, 180.0), NSSize::new(100.0, 100.0)),
    );

    if let Some(circle_image) =
        NSImage::imageWithSystemSymbolName_accessibilityDescription(ns_string!("circle"), None)
    {
        circle_image.setSize(NSSize::new(80.0, 80.0));
        image_view.setImage(Some(&circle_image));
    }
    image_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    content_view.addSubview(&image_view);
}

/// Adds the app name label to the window
fn add_app_name_label(content_view: &objc2_app_kit::NSView, mtm: MainThreadMarker) {
    let app_name_label = NSTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(20.0, 140.0), NSSize::new(260.0, 30.0)),
    );
    let app_name = NSString::from_str("Something in the Background");
    app_name_label.setStringValue(&app_name);
    app_name_label.setEditable(false);
    app_name_label.setBordered(false);
    app_name_label.setDrawsBackground(false);
    app_name_label.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    let bold_font = objc2_app_kit::NSFont::boldSystemFontOfSize(16.0);
    app_name_label.setFont(Some(&bold_font));
    content_view.addSubview(&app_name_label);
}

/// Adds the version label to the window
fn add_version_label(content_view: &objc2_app_kit::NSView, mtm: MainThreadMarker) {
    let version_label = NSTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(20.0, 110.0), NSSize::new(260.0, 25.0)),
    );
    let version_text = NSString::from_str(&format!("Version {}", get_app_version()));
    version_label.setStringValue(&version_text);
    version_label.setEditable(false);
    version_label.setBordered(false);
    version_label.setDrawsBackground(false);
    version_label.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    content_view.addSubview(&version_label);
}

/// Adds the GitHub button to the window
fn add_github_button(
    content_view: &objc2_app_kit::NSView,
    url_helper: &URLButtonHelper,
    mtm: MainThreadMarker,
) {
    let github_button = NSButton::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(50.0, 65.0), NSSize::new(200.0, 30.0)),
    );
    let github_url_text = NSString::from_str("View on GitHub");
    github_button.setTitle(&github_url_text);
    github_button.setBezelStyle(objc2_app_kit::NSBezelStyle::Push);

    // SAFETY: url_helper is a valid target that responds to the openURL: selector
    unsafe {
        github_button.setTarget(Some(url_helper as &AnyObject));
        github_button.setAction(Some(sel!(openURL:)));
    }
    content_view.addSubview(&github_button);
}
