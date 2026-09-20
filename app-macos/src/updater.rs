//! Sparkle updater bridge for the macOS app shell.
//!
//! Sparkle is loaded dynamically from the application bundle so ordinary
//! `cargo run` development builds do not need to link or bundle the framework.

use objc2::{
    ClassType, MainThreadOnly, define_class,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
use std::cell::RefCell;
use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const SPARKLE_UNAVAILABLE_MESSAGE: &str = "Sparkle updates are unavailable in this build. Bundle Sparkle.framework in Contents/Frameworks and configure SUFeedURL and SUPublicEDKey.";

struct UpdaterState {
    controller: Retained<AnyObject>,
    _delegate: Retained<UpdaterDelegate>,
}

thread_local! {
    // Sparkle's controller is main-thread-only. Thread-local storage both keeps
    // Objective-C objects alive and prevents accidental cross-thread access.
    static UPDATER_STATE: RefCell<Option<UpdaterState>> = const { RefCell::new(None) };
}

static AUTOMATIC_UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SomethingBgSparkleDelegate"]
    struct UpdaterDelegate;

    unsafe impl NSObjectProtocol for UpdaterDelegate {}

    impl UpdaterDelegate {
        #[unsafe(method(updater:didFindValidUpdate:))]
        fn did_find_valid_update(&self, _updater: &NSObject, _item: &NSObject) {
            AUTOMATIC_UPDATE_AVAILABLE.store(true, Ordering::SeqCst);
        }

        #[unsafe(method(updater:didFinishLoadingAppcast:))]
        fn did_finish_loading_appcast(&self, updater: &NSObject, appcast: &NSObject) {
            // Scheduled results exclude skipped versions. The menu describes
            // availability, independently of Sparkle's reminder/skip decisions.
            if let Some(available) = appcast_has_available_update(updater, appcast) {
                AUTOMATIC_UPDATE_AVAILABLE.store(available, Ordering::SeqCst);
            }
        }

        #[unsafe(method(supportsGentleScheduledUpdateReminders))]
        fn supports_gentle_scheduled_update_reminders(&self) -> bool {
            true
        }

        #[unsafe(method(standardUserDriverShouldHandleShowingScheduledUpdate:andInImmediateFocus:))]
        fn should_sparkle_show_scheduled_update(
            &self,
            _update: &NSObject,
            _immediate_focus: bool,
        ) -> bool {
            // Keep scheduled checks quiet, including at login. A notification and
            // the menu action let the user choose when to open the prepared alert.
            false
        }

        #[unsafe(method(standardUserDriverWillHandleShowingUpdate:forUpdate:state:))]
        fn will_handle_showing_update(
            &self,
            sparkle_will_show: bool,
            update: &NSObject,
            state: &NSObject,
        ) {
            let user_initiated: bool = unsafe { objc2::msg_send![state, userInitiated] };
            if !sparkle_will_show && !user_initiated {
                let version: Retained<NSString> =
                    unsafe { objc2::msg_send![update, displayVersionString] };
                crate::notifications::send_update_notification(&version.to_string());
            }
        }

        #[unsafe(method(standardUserDriverDidReceiveUserAttentionForUpdate:))]
        fn did_receive_update_attention(&self, _update: &NSObject) {
            crate::notifications::clear_update_notification();
        }

        #[unsafe(method(standardUserDriverWillFinishUpdateSession))]
        fn will_finish_update_session(&self) {
            crate::notifications::clear_update_notification();
        }
    }
);

impl UpdaterDelegate {
    fn new(_mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { objc2::msg_send![Self::class(), new] }
    }
}

/// Starts Sparkle and performs a launch-time background check when
/// automatic checks are enabled in the bundle.
pub fn start_automatic_checks() -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "Sparkle must be initialized on the main thread.".to_string())?;
    ensure_controller(mtm)?;

    UPDATER_STATE.with(|state| {
        let state = state.borrow();
        let controller = &state
            .as_ref()
            .ok_or_else(|| "Sparkle updater state was not retained.".to_string())?
            .controller;
        unsafe {
            let updater: Retained<AnyObject> = objc2::msg_send![controller, updater];
            let automatic_checks: bool = objc2::msg_send![&updater, automaticallyChecksForUpdates];
            if automatic_checks {
                let _: () = objc2::msg_send![&updater, checkForUpdatesInBackground];
            }
        }
        Ok(())
    })
}

/// Opens Sparkle's standard, user-initiated update flow.
pub fn check_for_updates() -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "Sparkle update checks must run on the main thread.".to_string())?;
    ensure_controller(mtm)?;

    UPDATER_STATE.with(|state| {
        let state = state.borrow();
        let controller = &state
            .as_ref()
            .ok_or_else(|| "Sparkle updater state was not retained.".to_string())?
            .controller;
        unsafe {
            let _: () =
                objc2::msg_send![controller, checkForUpdates: std::ptr::null::<AnyObject>()];
        }
        Ok(())
    })
}

/// Whether the Sparkle runtime has initialized successfully in this process.
pub fn is_available() -> bool {
    UPDATER_STATE.with(|state| state.borrow().is_some())
}

/// Whether Sparkle can currently begin or focus a user-initiated check.
pub fn can_check_for_updates() -> bool {
    UPDATER_STATE.with(|state| {
        let state = state.borrow();
        let Some(controller) = state.as_ref().map(|state| &state.controller) else {
            return false;
        };
        unsafe {
            let updater: Retained<AnyObject> = objc2::msg_send![controller, updater];
            objc2::msg_send![&updater, canCheckForUpdates]
        }
    })
}

/// Whether the feed contains an available update, even if dismissed or skipped.
pub fn automatic_update_available() -> bool {
    AUTOMATIC_UPDATE_AVAILABLE.load(Ordering::SeqCst)
}

/// Use Sparkle's parsed eligibility flags and version comparator for the menu
/// hint. Sparkle still chooses which update to offer and when to show its UI.
/// Inspecting the loaded feed also restores the hint after restarting with a
/// skipped version, without storing a second copy of Sparkle's preferences.
fn appcast_has_available_update(updater: &NSObject, appcast: &NSObject) -> Option<bool> {
    let comparator_class = AnyClass::get(c"SUStandardVersionComparator")?;
    unsafe {
        let comparator: Retained<AnyObject> = objc2::msg_send![comparator_class, defaultComparator];
        let bundle: Retained<AnyObject> = objc2::msg_send![updater, hostBundle];
        let installed_version: Option<Retained<NSString>> = objc2::msg_send![
            &bundle, objectForInfoDictionaryKey: &*NSString::from_str("CFBundleVersion")
        ];
        let installed_version = installed_version?;
        let items: Retained<AnyObject> = objc2::msg_send![appcast, items];
        let count: usize = objc2::msg_send![&items, count];
        for index in 0..count {
            let item: Retained<AnyObject> = objc2::msg_send![&items, objectAtIndex: index];
            let macos: bool = objc2::msg_send![&item, isMacOsUpdate];
            let delta: bool = objc2::msg_send![&item, isDeltaUpdate];
            let channel: Option<Retained<NSString>> = objc2::msg_send![&item, channel];
            let minimum_update: bool = objc2::msg_send![&item, minimumUpdateVersionIsOK];
            let minimum_os: bool = objc2::msg_send![&item, minimumOperatingSystemVersionIsOK];
            let maximum_os: bool = objc2::msg_send![&item, maximumOperatingSystemVersionIsOK];
            let hardware: bool = objc2::msg_send![&item, arm64HardwareRequirementIsOK];
            // This app subscribes only to Sparkle's default (stable) channel.
            if !macos
                || delta
                || channel.is_some()
                || !minimum_update
                || !minimum_os
                || !maximum_os
                || !hardware
            {
                continue;
            }
            let version: Retained<NSString> = objc2::msg_send![&item, versionString];
            let comparison: isize = objc2::msg_send![
                &comparator, compareVersion: &*installed_version, toVersion: &*version
            ];
            if comparison < 0 {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// Derives the status-menu presentation without duplicating updater state in
/// the AppKit menu layer.
pub(crate) fn menu_item_presentation() -> (&'static str, bool) {
    menu_item_presentation_for(
        is_available(),
        can_check_for_updates(),
        automatic_update_available(),
    )
}

fn menu_item_presentation_for(
    runtime_available: bool,
    can_check: bool,
    update_available: bool,
) -> (&'static str, bool) {
    let title = if runtime_available && update_available {
        "Update Available..."
    } else {
        "Check for Updates..."
    };
    (title, runtime_available && can_check)
}

fn ensure_controller(mtm: MainThreadMarker) -> Result<(), String> {
    if is_available() {
        return Ok(());
    }

    load_sparkle_framework()?;
    let controller_class = AnyClass::get(c"SPUStandardUpdaterController")
        .ok_or_else(|| SPARKLE_UNAVAILABLE_MESSAGE.to_string())?;
    let delegate = UpdaterDelegate::new(mtm);
    let controller = unsafe {
        let allocated: *mut AnyObject = objc2::msg_send![controller_class, alloc];
        let initialized: *mut AnyObject = objc2::msg_send![
            allocated,
            initWithStartingUpdater: true,
            updaterDelegate: &*delegate,
            userDriverDelegate: &*delegate
        ];
        Retained::from_raw(initialized)
    };
    let controller =
        controller.ok_or_else(|| "Sparkle updater failed to initialize.".to_string())?;

    UPDATER_STATE.with(|state| {
        *state.borrow_mut() = Some(UpdaterState {
            controller,
            _delegate: delegate,
        });
    });
    Ok(())
}

fn load_sparkle_framework() -> Result<(), String> {
    if AnyClass::get(c"SPUStandardUpdaterController").is_some() {
        return Ok(());
    }

    let path = bundled_sparkle_binary().ok_or_else(|| SPARKLE_UNAVAILABLE_MESSAGE.to_string())?;
    let path_string = path.to_str().ok_or_else(|| {
        format!(
            "Sparkle framework path is not valid UTF-8: {}",
            path.display()
        )
    })?;
    let path_c = CString::new(path_string).map_err(|_| {
        format!(
            "Sparkle framework path contains a null byte: {}",
            path.display()
        )
    })?;

    let handle = unsafe { libc::dlopen(path_c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    if handle.is_null() {
        let detail = unsafe {
            let error = libc::dlerror();
            if error.is_null() {
                "unknown loader error".to_string()
            } else {
                std::ffi::CStr::from_ptr(error)
                    .to_string_lossy()
                    .into_owned()
            }
        };
        return Err(format!(
            "Failed to load Sparkle.framework from '{}': {detail}",
            path.display()
        ));
    }

    if AnyClass::get(c"SPUStandardUpdaterController").is_none() {
        return Err(format!(
            "Sparkle.framework loaded from '{}' but SPUStandardUpdaterController was not registered.",
            path.display()
        ));
    }
    Ok(())
}

fn bundled_sparkle_binary() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let contents = executable.parent()?.parent()?;
    let candidate = contents
        .join("Frameworks")
        .join("Sparkle.framework")
        .join("Sparkle");
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_message_is_actionable() {
        assert!(SPARKLE_UNAVAILABLE_MESSAGE.contains("Sparkle.framework"));
        assert!(SPARKLE_UNAVAILABLE_MESSAGE.contains("SUFeedURL"));
        assert!(SPARKLE_UNAVAILABLE_MESSAGE.contains("SUPublicEDKey"));
    }

    #[test]
    fn automatic_update_signal_tracks_state() {
        AUTOMATIC_UPDATE_AVAILABLE.store(false, Ordering::SeqCst);
        assert!(!automatic_update_available());
        AUTOMATIC_UPDATE_AVAILABLE.store(true, Ordering::SeqCst);
        assert!(automatic_update_available());
        AUTOMATIC_UPDATE_AVAILABLE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn menu_item_is_disabled_without_sparkle() {
        assert_eq!(
            menu_item_presentation_for(false, false, false),
            ("Check for Updates...", false)
        );
    }

    #[test]
    fn menu_item_allows_a_normal_manual_check() {
        assert_eq!(
            menu_item_presentation_for(true, true, false),
            ("Check for Updates...", true)
        );
    }

    #[test]
    fn menu_item_advertises_a_discovered_update() {
        assert_eq!(
            menu_item_presentation_for(true, true, true),
            ("Update Available...", true)
        );
    }
}
