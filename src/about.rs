// src/about.rs
//
// Handles the About window display and related functionality.

use log::{error, info};
use objc2::{
    ClassType, MainThreadOnly, define_class, rc::Retained, runtime::AnyObject, sel,
};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSImage, NSImageScaling, NSImageView, NSTextField, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL,
    ns_string,
};

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

    unsafe {
        // Create window
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(300.0, 280.0));
        let style_mask = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;

        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame,
            style_mask,
            NSBackingStoreType::Buffered,
            false,
        );

        window.center();

        // Get content view
        let content_view = window.contentView().unwrap();

        // Create circle image (using SF Symbol or drawing)
        let image_view = NSImageView::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::new(100.0, 180.0), NSSize::new(100.0, 100.0)),
        );

        // Use SF Symbol for circle ring (stroke only)
        if let Some(circle_image) =
            NSImage::imageWithSystemSymbolName_accessibilityDescription(ns_string!("circle"), None)
        {
            circle_image.setSize(NSSize::new(80.0, 80.0));
            image_view.setImage(Some(&circle_image));
        }
        image_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        content_view.addSubview(&image_view);

        // App name label
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

        // Version label
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

        // Create a clickable button for GitHub URL
        let github_button = NSButton::initWithFrame(
            mtm.alloc(),
            NSRect::new(NSPoint::new(50.0, 65.0), NSSize::new(200.0, 30.0)),
        );
        let github_url_text = NSString::from_str("View on GitHub");
        github_button.setTitle(&github_url_text);
        github_button.setBezelStyle(objc2_app_kit::NSBezelStyle::Push);

        // Create helper for button action and set it as target
        let url_helper = URLButtonHelper::new(mtm);
        github_button.setTarget(Some(&url_helper as &AnyObject));
        github_button.setAction(Some(sel!(openURL:)));
        content_view.addSubview(&github_button);

        // Keep helper alive
        std::mem::forget(url_helper);

        // Make window float above other windows
        window.setLevel(objc2_app_kit::NSFloatingWindowLevel);

        // Show the window and bring it to front
        window.makeKeyAndOrderFront(None);

        // Activate the application to ensure window is visible
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        app.activate();

        // Keep window alive by storing it globally (it will be released when closed)
        // For a simple approach, we'll use NSApplication's addWindowsItem or similar
        // But since this is a one-off window, we need to prevent it from being deallocated
        std::mem::forget(window);
    }
}
