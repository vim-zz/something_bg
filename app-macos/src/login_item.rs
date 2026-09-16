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
    Enabled,
    RequiresApproval,
}

impl Status {
    pub fn requested(self) -> bool {
        self != Self::Off
    }

    pub fn menu_state(self) -> isize {
        match self {
            Self::Off => 0,
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
    match status {
        0 => Ok(Status::Off),
        1 => Ok(Status::Enabled),
        2 => Ok(Status::RequiresApproval),
        _ => Err("macOS could not find this app's login item. Install the app in Applications and try again.".into()),
    }
}

pub fn status() -> Result<Status, String> {
    read_status(&*service()?)
}

pub fn set_enabled(enabled: bool) -> Result<Status, String> {
    let service = service()?;
    let before = read_status(&service)?;
    // Respect revoked consent: never unregister/re-register to bypass approval.
    if before.requested() == enabled {
        return Ok(before);
    }
    let result: Result<(), Retained<NSError>> = unsafe {
        if enabled {
            msg_send![&service, registerAndReturnError: _]
        } else {
            msg_send![&service, unregisterAndReturnError: _]
        }
    };
    let after = read_status(&service)?;
    // Registration may return launch-denied while retaining the approval request.
    if after.requested() == enabled {
        return Ok(after);
    }
    match result {
        Err(error) => Err(error.localizedDescription().to_string()),
        Ok(()) => Err("macOS did not apply the Start at Login change. Please try again.".into()),
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
    fn approval_is_requested_but_never_shown_as_enabled() {
        assert!(Status::RequiresApproval.requested());
        assert_eq!(Status::RequiresApproval.menu_state(), -1);
        assert_eq!(Status::Enabled.menu_state(), 1);
        assert!(!Status::Off.requested());
        assert_eq!(Status::Off.menu_state(), 0);
    }
}
