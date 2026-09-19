//! Native, selectable connection diagnostics. No commands are executed here.
use std::cell::RefCell;

use objc2::{ClassType, MainThreadOnly, define_class, rc::Retained, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua,
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSBorderType, NSBox, NSBoxType,
    NSButton, NSCellImagePosition, NSColor, NSFont, NSImage, NSPasteboard, NSPasteboardTypeString,
    NSScrollView, NSTextAlignment, NSTextField, NSTextView, NSTitlePosition, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer,
};

struct DetailsWindow {
    window: Retained<NSWindow>,
    // Button targets are weak; retain the handler for as long as the window.
    _handler: Retained<DetailsHandler>,
    command: String,
    path: String,
    logs: String,
}

const COPY_FEEDBACK_TAG: isize = 1;

thread_local! {
    static DETAILS: RefCell<Option<DetailsWindow>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ConnectionDetailsHandler"]
    struct DetailsHandler;

    unsafe impl NSObjectProtocol for DetailsHandler {}

    impl DetailsHandler {
        #[unsafe(method(copyCommand:))]
        fn copy_command(&self, sender: &NSButton) {
            DETAILS.with(|cell| {
                if let Some(details) = cell.borrow().as_ref() { self.show_copy_result(sender, copy(&details.command)); }
            });
        }

        #[unsafe(method(copyPath:))]
        fn copy_path(&self, sender: &NSButton) {
            DETAILS.with(|cell| {
                if let Some(details) = cell.borrow().as_ref() { self.show_copy_result(sender, copy(&details.path)); }
            });
        }

        #[unsafe(method(copyLogs:))]
        fn copy_logs(&self, sender: &NSButton) {
            DETAILS.with(|cell| {
                if let Some(details) = cell.borrow().as_ref() { self.show_copy_result(sender, copy(&details.logs)); }
            });
        }
        #[unsafe(method(resetCopy:))]
        fn reset_copy(&self, timer: &NSTimer) {
            if let Some(info) = timer.userInfo()
                && let Ok(button) = info.downcast::<NSButton>() {
                reset_copy_button(&button);
            }
        }
    }
);

impl DetailsHandler {
    fn show_copy_result(&self, button: &NSButton, success: bool) {
        // Keep one reset timer per button even if it is clicked repeatedly.
        if button.title().to_string() == "Copied" || button.title().to_string() == "Copy failed" {
            return;
        }
        let title = if success { "Copied" } else { "Copy failed" };
        button.setTitle(&NSString::from_str(title));
        button.setAccessibilityLabel(Some(&NSString::from_str(title)));
        button.setImage(
            symbol(if success {
                "checkmark"
            } else {
                "exclamationmark.triangle"
            })
            .as_deref(),
        );
        button.setImagePosition(NSCellImagePosition::ImageOnly);
        let tint = if success {
            color(0.60, 0.90, 0.72)
        } else {
            color(1.0, 0.68, 0.66)
        };
        button.setContentTintColor(Some(&tint));
        if let Some(label) = copy_feedback_label(button) {
            label.setStringValue(&NSString::from_str(title));
            label.setTextColor(Some(&tint));
            label.setHidden(false);
        }
        // NSTimer retains the target and button until the one-shot callback fires.
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                1.8,
                self,
                sel!(resetCopy:),
                Some(button),
                false,
            );
        }
    }
}

fn copy(text: &str) -> bool {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    unsafe { pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

pub fn show(
    name: &str,
    command: &str,
    path: &str,
    logs: &str,
    started_at: Option<&str>,
    failed_at: Option<&str>,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let handler: Retained<DetailsHandler> =
        unsafe { objc2::msg_send![DetailsHandler::class(), new] };
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect(0.0, 0.0, 760.0, 740.0),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str(&format!("{name} — Process Error")));
    window.setContentMinSize(NSSize::new(560.0, 600.0));
    let content = window.contentView().unwrap();
    for (index, time) in [started_at, failed_at].into_iter().flatten().enumerate() {
        let metadata = NSTextField::initWithFrame(
            mtm.alloc(),
            rect(20.0, 700.0 - index as f64 * 25.0, 720.0, 20.0),
        );
        metadata.setStringValue(&NSString::from_str(time));
        metadata.setEditable(false);
        metadata.setSelectable(true);
        metadata.setBordered(false);
        metadata.setDrawsBackground(false);
        metadata.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        metadata.setTextColor(Some(&NSColor::secondaryLabelColor()));
        metadata.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        content.addSubview(&metadata);
    }
    label(&content, "Command", rect(20.0, 631.0, 400.0, 20.0), mtm);
    text_area(
        &content,
        &handler,
        command,
        ("Copy Command", sel!(copyCommand:)),
        rect(20.0, 515.0, 720.0, 110.0),
        false,
        &color(0.90, 0.92, 0.96),
    );
    label(
        &content,
        "PATH (from settings)",
        rect(20.0, 476.0, 400.0, 20.0),
        mtm,
    );
    text_area(
        &content,
        &handler,
        path,
        ("Copy PATH", sel!(copyPath:)),
        rect(20.0, 385.0, 720.0, 85.0),
        false,
        &color(0.60, 0.90, 0.72),
    );
    label(&content, "Error logs", rect(20.0, 346.0, 400.0, 20.0), mtm);
    text_area(
        &content,
        &handler,
        logs,
        ("Copy Logs", sel!(copyLogs:)),
        rect(20.0, 20.0, 720.0, 320.0),
        true,
        &color(1.0, 0.75, 0.73),
    );
    window.center();
    DETAILS.with(|cell| {
        if let Some(previous) = cell.borrow_mut().take() {
            previous.window.close();
        }
        *cell.borrow_mut() = Some(DetailsWindow {
            window: window.clone(),
            _handler: handler,
            command: command.to_owned(),
            path: path.to_owned(),
            logs: [started_at, failed_at, Some(logs)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n"),
        });
    });
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(mtm).activate();
}

fn label(parent: &NSView, title: &str, frame: NSRect, mtm: MainThreadMarker) {
    let label = NSTextField::initWithFrame(mtm.alloc(), frame);
    label.setStringValue(&NSString::from_str(title));
    label.setEditable(false);
    label.setBordered(false);
    label.setDrawsBackground(false);
    label.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    label.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
    parent.addSubview(&label);
}

fn color(red: f64, green: f64, blue: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 1.0)
}

fn symbol(name: &str) -> Option<Retained<NSImage>> {
    NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
}

fn copy_feedback_label(button: &NSButton) -> Option<Retained<NSTextField>> {
    // The button and its panel are owned and accessed on the AppKit main thread.
    unsafe { button.superview() }?
        .viewWithTag(COPY_FEEDBACK_TAG)?
        .downcast()
        .ok()
}

fn reset_copy_button(button: &NSButton) {
    button.setTitle(
        &button
            .toolTip()
            .unwrap_or_else(|| NSString::from_str("Copy")),
    );
    button.setAccessibilityLabel(Some(&button.title()));
    if let Some(label) = copy_feedback_label(button) {
        label.setHidden(true);
    }
    let image = symbol("doc.on.doc");
    button.setImage(image.as_deref());
    button.setImagePosition(if image.is_some() {
        NSCellImagePosition::ImageOnly
    } else {
        NSCellImagePosition::NoImage
    });
    button.setContentTintColor(Some(&color(0.72, 0.77, 0.84)));
}

fn text_area(
    parent: &NSView,
    handler: &DetailsHandler,
    value: &str,
    copy_control: (&str, objc2::runtime::Sel),
    frame: NSRect,
    expand_height: bool,
    foreground: &NSColor,
) {
    let mtm = handler.mtm();
    let (copy_title, action) = copy_control;
    let background = color(0.10, 0.12, 0.15);
    let panel = NSBox::initWithFrame(mtm.alloc(), frame);
    panel.setBoxType(NSBoxType::Custom);
    panel.setTitlePosition(NSTitlePosition::NoTitle);
    panel.setBorderWidth(1.0);
    panel.setBorderColor(&color(0.24, 0.28, 0.34));
    panel.setCornerRadius(8.0);
    panel.setFillColor(&background);
    panel.setContentViewMargins(NSSize::new(0.0, 0.0));
    panel.setAppearance(
        NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref(),
    );
    panel.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable
            | if expand_height {
                NSAutoresizingMaskOptions::ViewHeightSizable
            } else {
                NSAutoresizingMaskOptions::ViewMinYMargin
            },
    );
    let interior = panel.contentView().unwrap();
    let bounds = interior.bounds();
    // Keep controls alongside the first line, with room for feedback at the right.
    let button = NSButton::initWithFrame(
        mtm.alloc(),
        rect(
            bounds.size.width - 38.0,
            bounds.size.height - 32.0,
            28.0,
            26.0,
        ),
    );
    button.setBordered(false);
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    button.setToolTip(Some(&NSString::from_str(copy_title)));
    reset_copy_button(&button);
    button.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    unsafe {
        button.setTarget(Some(handler));
        button.setAction(Some(action));
    }
    interior.addSubview(&button);
    let feedback = NSTextField::initWithFrame(
        mtm.alloc(),
        rect(
            bounds.size.width - 114.0,
            bounds.size.height - 29.0,
            72.0,
            20.0,
        ),
    );
    feedback.setTag(COPY_FEEDBACK_TAG);
    feedback.setEditable(false);
    feedback.setSelectable(false);
    feedback.setBordered(false);
    feedback.setDrawsBackground(false);
    feedback.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    feedback.setAlignment(NSTextAlignment::Right);
    feedback.setHidden(true);
    feedback.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    interior.addSubview(&feedback);
    let scroll = NSScrollView::initWithFrame(
        mtm.alloc(),
        rect(
            10.0,
            10.0,
            bounds.size.width - 128.0,
            bounds.size.height - 20.0,
        ),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setBackgroundColor(&background);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let size = scroll.contentSize();
    let text = NSTextView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, size.width, size.height));
    text.setEditable(false);
    text.setSelectable(true);
    text.setRichText(false);
    text.setBackgroundColor(&background);
    text.setTextColor(Some(foreground));
    text.setTextContainerInset(NSSize::new(2.0, 2.0));
    text.setFont(NSFont::userFixedPitchFontOfSize(12.0).as_deref());
    text.setMinSize(NSSize::new(0.0, size.height));
    text.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    text.setVerticallyResizable(true);
    text.setHorizontallyResizable(false);
    text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    // The text view and its owned text container are accessed on the main thread.
    if let Some(container) = unsafe { text.textContainer() } {
        container.setContainerSize(NSSize::new(size.width, f64::MAX));
        container.setWidthTracksTextView(true);
    }
    text.setString(&NSString::from_str(value));
    scroll.setDocumentView(Some(&text));
    interior.addSubview(&scroll);
    parent.addSubview(&panel);
}
