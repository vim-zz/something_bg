//! Native, selectable connection diagnostics. No commands are executed here.
use std::cell::RefCell;

use objc2::{ClassType, MainThreadOnly, define_class, rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSBezelStyle, NSButton, NSFont,
    NSPasteboard, NSPasteboardTypeString, NSScrollView, NSTextField, NSTextView, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

struct DetailsWindow {
    window: Retained<NSWindow>,
    // Button targets are weak; retain the handler for as long as the window.
    _handler: Retained<DetailsHandler>,
    command: String,
    logs: String,
}

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
        fn copy_command(&self, _sender: &AnyObject) {
            DETAILS.with(|cell| {
                if let Some(details) = cell.borrow().as_ref() { copy(&details.command); }
            });
        }

        #[unsafe(method(copyLogs:))]
        fn copy_logs(&self, _sender: &AnyObject) {
            DETAILS.with(|cell| {
                if let Some(details) = cell.borrow().as_ref() { copy(&details.logs); }
            });
        }
    }
);

fn copy(text: &str) {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    unsafe {
        pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

pub fn show(name: &str, command: &str, logs: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let handler: Retained<DetailsHandler> =
        unsafe { objc2::msg_send![DetailsHandler::class(), new] };
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect(0.0, 0.0, 760.0, 560.0),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str(&format!("{name} — Connection Error")));
    window.setContentMinSize(NSSize::new(560.0, 420.0));
    let content = window.contentView().unwrap();
    label(&content, "Command", rect(20.0, 519.0, 400.0, 24.0), mtm);
    button(
        &content,
        &handler,
        "Copy Command",
        sel!(copyCommand:),
        rect(590.0, 515.0, 150.0, 30.0),
        mtm,
    );
    text_area(
        &content,
        command,
        rect(20.0, 395.0, 720.0, 110.0),
        false,
        mtm,
    );
    label(&content, "Error logs", rect(20.0, 354.0, 400.0, 24.0), mtm);
    button(
        &content,
        &handler,
        "Copy Logs",
        sel!(copyLogs:),
        rect(590.0, 350.0, 150.0, 30.0),
        mtm,
    );
    text_area(&content, logs, rect(20.0, 20.0, 720.0, 320.0), true, mtm);
    window.center();
    DETAILS.with(|cell| {
        if let Some(previous) = cell.borrow_mut().take() {
            previous.window.close();
        }
        *cell.borrow_mut() = Some(DetailsWindow {
            window: window.clone(),
            _handler: handler,
            command: command.to_owned(),
            logs: logs.to_owned(),
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

fn button(
    parent: &NSView,
    handler: &DetailsHandler,
    title: &str,
    action: objc2::runtime::Sel,
    frame: NSRect,
    mtm: MainThreadMarker,
) {
    let button = NSButton::initWithFrame(mtm.alloc(), frame);
    button.setTitle(&NSString::from_str(title));
    button.setBezelStyle(NSBezelStyle::Push);
    button.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    unsafe {
        button.setTarget(Some(handler));
        button.setAction(Some(action));
    }
    parent.addSubview(&button);
}

fn text_area(
    parent: &NSView,
    value: &str,
    frame: NSRect,
    expand_height: bool,
    mtm: MainThreadMarker,
) {
    let scroll = NSScrollView::initWithFrame(mtm.alloc(), frame);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setBorderType(objc2_app_kit::NSBorderType::BezelBorder);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable
            | if expand_height {
                NSAutoresizingMaskOptions::ViewHeightSizable
            } else {
                NSAutoresizingMaskOptions::ViewMinYMargin
            },
    );
    let size = scroll.contentSize();
    let text = NSTextView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, size.width, size.height));
    text.setEditable(false);
    text.setSelectable(true);
    text.setRichText(false);
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
    parent.addSubview(&scroll);
}
