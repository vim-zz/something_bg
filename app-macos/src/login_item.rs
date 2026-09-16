//! Native macOS login-item registration. No helper or launch-agent file needed.
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{MainThreadMarker, NSBundle, NSError};

// The framework exists on older macOS releases; look up the macOS 13 class at runtime.
#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Off,
    NotFound,
    Enabled,
    RequiresApproval,
}

impl Status {
    pub fn requested(self) -> bool {
        matches!(self, Self::Enabled | Self::RequiresApproval)
    }

    pub fn menu_state(self) -> isize {
        match self {
            Self::Off | Self::NotFound => 0,
            Self::Enabled => 1,
            Self::RequiresApproval => -1,
        }
    }
}

fn service_class() -> Result<&'static AnyClass, String> {
    MainThreadMarker::new().ok_or("Login items must be managed on the main thread.")?;
    let bundle = NSBundle::mainBundle();
    if !bundle.bundlePath().to_string().ends_with(".app")
        || bundle
            .bundleIdentifier()
            .as_deref()
            .map(ToString::to_string)
            .as_deref()
            != Some("com.vim-zz.something-bg")
    {
        return Err("Start at Login requires the installed app bundle; it is unavailable in cargo run builds.".into());
    }
    AnyClass::get(c"SMAppService")
        .ok_or_else(|| "Start at Login requires macOS 13 or later.".into())
}

fn service() -> Result<Retained<AnyObject>, String> {
    let class = service_class()?;
    // SAFETY: selectors and NSInteger/BOOL signatures come from SMAppService.h.
    Ok(unsafe { msg_send![class, mainAppService] })
}

fn read_status(service: &AnyObject) -> Result<Status, String> {
    let status: isize = unsafe { msg_send![service, status] };
    status_from_raw(status)
}

fn status_from_raw(status: isize) -> Result<Status, String> {
    match status {
        0 => Ok(Status::Off),
        1 => Ok(Status::Enabled),
        2 => Ok(Status::RequiresApproval),
        // A missing service is not evidence that the app is installed incorrectly.
        // It is inactive, and must not prevent an explicit registration attempt.
        3 => Ok(Status::NotFound),
        other => Err(format!(
            "macOS returned an unknown login-item status ({other})."
        )),
    }
}

pub fn status() -> Result<Status, String> {
    read_status(&*service()?)
}

pub fn set_enabled(enabled: bool) -> Result<Status, String> {
    let service = service()?;
    apply_registration(
        enabled,
        || read_status(&service),
        |enabled| {
            let result: Result<(), Retained<NSError>> = unsafe {
                if enabled {
                    msg_send![&service, registerAndReturnError: _]
                } else {
                    msg_send![&service, unregisterAndReturnError: _]
                }
            };
            result.map_err(|error| error.localizedDescription().to_string())
        },
    )
}

fn apply_registration(
    enabled: bool,
    mut read: impl FnMut() -> Result<Status, String>,
    mut change: impl FnMut(bool) -> Result<(), String>,
) -> Result<Status, String> {
    let before = read()?;
    // Missing + off is already satisfied. Enabling a missing item must reach
    // register(), while existing approval requests must not be bypassed.
    if before.requested() == enabled {
        return Ok(before);
    }
    let result = change(enabled);
    match (result, read()) {
        // Registration can return launch-denied while retaining the request.
        (_, Ok(after)) if after.requested() == enabled => Ok(after),
        // Prefer the actual registration failure over a secondary status error.
        (Err(error), _) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(_)) => {
            Err("macOS did not apply the Start at Login change. Please try again.".into())
        }
    }
}

/// Older OS versions and unbundled development runs can use the app with the
/// default (off) preference without generating a startup error.
pub fn apply_preference(enabled: bool) -> Result<Status, String> {
    if !enabled && service_class().is_err() {
        return Ok(Status::Off);
    }
    set_enabled(enabled)
}

pub fn open_settings() -> Result<(), String> {
    let class = service_class()?;
    unsafe {
        let _: () = msg_send![class, openSystemSettingsLoginItems];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_item_with_default_off_does_not_register_or_raise_an_error() {
        let missing = status_from_raw(3).unwrap();
        assert_eq!(missing.menu_state(), 0);
        assert!(!missing.requested());
        let result = apply_registration(
            false,
            || Ok(missing),
            |_| panic!("Off must not change registration"),
        );
        assert_eq!(result, Ok(Status::NotFound));
    }

    #[test]
    fn missing_item_can_be_registered_from_the_menu_or_config() {
        let state = std::cell::Cell::new(Status::NotFound);
        let calls = std::cell::Cell::new(0);
        let result = apply_registration(
            true,
            || Ok(state.get()),
            |enabled| {
                assert!(enabled);
                calls.set(calls.get() + 1);
                state.set(Status::Enabled);
                Ok(())
            },
        );
        assert_eq!(result, Ok(Status::Enabled));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn registration_failure_preserves_macos_error_even_when_status_read_fails() {
        let reads = std::cell::Cell::new(0);
        let result = apply_registration(
            true,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 {
                    Ok(Status::NotFound)
                } else {
                    Err("status failed".into())
                }
            },
            |_| Err("The code signature is invalid.".into()),
        );
        assert_eq!(result.unwrap_err(), "The code signature is invalid.");
    }

    #[test]
    fn unsuccessful_registration_is_not_reported_as_enabled() {
        let result = apply_registration(true, || Ok(Status::NotFound), |_| Ok(()));
        assert!(result.is_err());
    }

    #[test]
    fn enabling_does_not_bypass_existing_user_approval() {
        let result = apply_registration(
            true,
            || Ok(Status::RequiresApproval),
            |_| panic!("Must not re-register to bypass consent"),
        );
        assert_eq!(result, Ok(Status::RequiresApproval));
    }

    #[test]
    fn disabling_removes_an_existing_registration() {
        let state = std::cell::Cell::new(Status::Enabled);
        let result = apply_registration(
            false,
            || Ok(state.get()),
            |enabled| {
                assert!(!enabled);
                state.set(Status::Off);
                Ok(())
            },
        );
        assert_eq!(result, Ok(Status::Off));
    }

    #[test]
    fn approval_is_requested_but_never_shown_as_enabled() {
        assert!(Status::RequiresApproval.requested());
        assert_eq!(Status::RequiresApproval.menu_state(), -1);
        assert_eq!(Status::Enabled.menu_state(), 1);
        assert!(!Status::Off.requested());
        assert_eq!(Status::Off.menu_state(), 0);
    }
}
