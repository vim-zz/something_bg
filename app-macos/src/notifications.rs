//! Native notification delivery and actions. Use only UserNotifications: macOS
//! rejects legacy NSUserNotificationCenter clients once an app has migrated.
use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use log::{info, warn};
use objc2::{
    AnyThread, define_class,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSSet,
    NSString, NSUUID,
};
use objc2_user_notifications::*;
use something_bg_core::tunnel::TunnelFailure;

#[derive(Debug, PartialEq, Eq)]
enum Destination {
    History,
    LoginSettings,
    Failure {
        name: String,
        command: String,
        path: String,
        logs: String,
        started_at: Option<String>,
        failed_at: Option<String>,
    },
}

impl Destination {
    fn category(&self) -> &'static str {
        match self {
            Self::History => "command-history",
            Self::LoginSettings => "login-settings",
            Self::Failure { .. } => "failure-details",
        }
    }

    fn details(&self) -> Retained<NSDictionary> {
        let pairs: Vec<(&str, &str)> = match self {
            Self::History => vec![],
            Self::LoginSettings => vec![("loginItems", "true")],
            Self::Failure {
                name,
                command,
                path,
                logs,
                started_at,
                failed_at,
            } => {
                let mut pairs = vec![
                    ("tunnelName", name),
                    ("command", command),
                    ("path", path),
                    ("logs", logs),
                ];
                if let Some(time) = started_at {
                    pairs.push(("startedAt", time));
                }
                if let Some(time) = failed_at {
                    pairs.push(("failedAt", time));
                }
                pairs
                    .into_iter()
                    .map(|(key, value)| (key, value.as_str()))
                    .collect()
            }
        };
        let keys: Vec<_> = pairs
            .iter()
            .map(|(key, _)| NSString::from_str(key))
            .collect();
        let values: Vec<_> = pairs
            .iter()
            .map(|(_, value)| NSString::from_str(value))
            .collect();
        let details = NSDictionary::from_slices(
            &keys.iter().map(|key| &**key).collect::<Vec<_>>(),
            &values.iter().map(|value| &**value).collect::<Vec<_>>(),
        );
        // Erase generic types; every key and value remains an NSString.
        unsafe { Retained::cast_unchecked(details) }
    }

    fn from_details(details: &NSDictionary) -> Self {
        let string = |key: &str| {
            details
                .objectForKey(&NSString::from_str(key))
                .and_then(|value| value.downcast_ref::<NSString>().map(ToString::to_string))
        };
        if string("loginItems").is_some() {
            return Self::LoginSettings;
        }
        if let (Some(name), Some(command), Some(logs)) =
            (string("tunnelName"), string("command"), string("logs"))
        {
            return Self::Failure {
                name,
                command,
                logs,
                started_at: string("startedAt"),
                failed_at: string("failedAt"),
                path: string("path")
                    .unwrap_or_else(|| "(not recorded for this notification)".to_owned()),
            };
        }
        Self::History
    }

    fn open(self) {
        // UserNotifications invokes its delegate on a background queue. AppKit
        // windows and Login Items settings must be handled on the main thread.
        DispatchQueue::main().exec_async(move || match self {
            Self::LoginSettings => {
                if let Err(error) = crate::login_item::open_settings() {
                    warn!("Could not open Login Items settings: {error}");
                }
            }
            Self::Failure {
                name,
                command,
                path,
                logs,
                started_at,
                failed_at,
            } => {
                crate::connection_details::show(
                    &name,
                    &command,
                    &path,
                    &logs,
                    started_at.as_deref(),
                    failed_at.as_deref(),
                );
            }
            Self::History => {
                if let Some(app) = crate::GLOBAL_APP.get()
                    && let Some(path) = app
                        .command_runner
                        .lock()
                        .unwrap()
                        .history_path()
                        .map(ToOwned::to_owned)
                    && path.exists()
                {
                    let _ = std::process::Command::new("open").arg(path).spawn();
                }
            }
        });
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            if response.actionIdentifier().to_string()
                != unsafe { UNNotificationDismissActionIdentifier }.to_string()
            {
                Destination::from_details(&response.notification().request().content().userInfo())
                    .open();
            }
            completion.call(());
        }
    }
);

fn center() -> Option<Retained<UNUserNotificationCenter>> {
    // The framework raises an Objective-C exception for unbundled cargo-run binaries.
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        warn!("Native notifications require an app bundle");
        return None;
    }
    Some(UNUserNotificationCenter::currentNotificationCenter())
}

pub fn setup_notification_center(_mtm: MainThreadMarker) {
    let Some(center) = center() else {
        return;
    };
    let delegate: Retained<NotificationDelegate> =
        unsafe { objc2::msg_send![NotificationDelegate::alloc(), init] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The notification center holds a weak delegate; retain it for the app lifetime.
    std::mem::forget(delegate);
    let categories: Vec<_> = [
        ("command-history", "View History"),
        ("failure-details", "View Details"),
        ("login-settings", "Open Settings"),
    ]
    .into_iter()
    .map(|(identifier, label)| {
        let action = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(identifier),
            &NSString::from_str(label),
            UNNotificationActionOptions::Foreground,
        );
        UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(identifier),
            &NSArray::from_slice(&[&*action]),
            &NSArray::new(),
            UNNotificationCategoryOptions::empty(),
        )
    })
    .collect();
    center.setNotificationCategories(&NSSet::from_slice(
        &categories
            .iter()
            .map(|category| &**category)
            .collect::<Vec<_>>(),
    ));
    info!("UserNotifications center configured");
}

pub fn send_notification(title: &str, body: &str) {
    deliver_notification(title, body, Destination::History);
}

pub fn send_login_notification(body: &str) {
    deliver_notification("Start at Login", body, Destination::LoginSettings);
}

pub fn send_tunnel_failure(name: &str, failure: &TunnelFailure) {
    send_process_failure(&format!("{name} — Faulty"), name, failure);
}

pub fn send_command_failure(name: &str, failure: &TunnelFailure) {
    send_process_failure(&format!("{name} — Failed"), name, failure);
}

fn send_process_failure(title: &str, name: &str, failure: &TunnelFailure) {
    deliver_notification(
        title,
        &format!(
            "{}. {} View Details for the command and error logs.",
            failure.failure_time_label(),
            failure.summary,
        ),
        Destination::Failure {
            name: name.to_owned(),
            command: failure.command_line.clone(),
            path: failure.env_path.clone(),
            logs: failure.error_logs(),
            started_at: Some(failure.start_time_label()),
            failed_at: Some(failure.failure_time_label()),
        },
    );
}

fn make_request(
    title: &str,
    body: &str,
    destination: &Destination,
) -> Retained<UNNotificationRequest> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setCategoryIdentifier(&NSString::from_str(destination.category()));
    // The payload contains only property-list-compatible string keys and values.
    unsafe {
        content.setUserInfo(&destination.details());
    }
    UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSUUID::UUID().UUIDString(),
        &content,
        None,
    )
}

fn deliver_notification(title: &str, body: &str, destination: Destination) {
    let Some(center) = center() else {
        return;
    };
    let request = make_request(title, body, &destination);
    // Wait for authorization before submitting, including the user's first alert.
    // Once decided, macOS returns the existing permission without another prompt.
    let authorization = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        if let Some(error) = unsafe { error.as_ref() } {
            warn!("Notification authorization failed: {error}");
            return;
        }
        if !granted.as_bool() {
            warn!("Notifications are disabled in macOS settings");
            return;
        }
        let identifier = request.identifier().to_string();
        let completion = RcBlock::new(move |error: *mut NSError| {
            if let Some(error) = unsafe { error.as_ref() } {
                warn!("Notification {identifier} failed: {error}");
            } else {
                info!("Notification {identifier} submitted");
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&completion));
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert,
        &authorization,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_notification_keeps_its_original_details_after_a_retry() {
        let original = Destination::Failure {
            name: "Test".into(),
            command: "false".into(),
            path: "/original/path".into(),
            logs: "Original failure".into(),
            started_at: Some("Started at 2026-09-19 14:00:00 +03:00".into()),
            failed_at: Some("Failed at 2026-09-19 14:05:09 +03:00".into()),
        };
        let first = make_request("Failed", "First attempt", &original);
        let second = make_request(
            "Failed",
            "Retry",
            &Destination::Failure {
                name: "Test".into(),
                command: "other".into(),
                path: "/new/path".into(),
                logs: "New failure".into(),
                started_at: Some("Started at 2026-09-19 14:09:00 +03:00".into()),
                failed_at: Some("Failed at 2026-09-19 14:10:00 +03:00".into()),
            },
        );
        assert_ne!(
            first.identifier().to_string(),
            second.identifier().to_string()
        );
        assert_eq!(
            Destination::from_details(&first.content().userInfo()),
            original
        );
        assert_eq!(
            first.content().categoryIdentifier().to_string(),
            "failure-details"
        );
        assert!(first.trigger().is_none());
    }

    #[test]
    fn notification_actions_preserve_login_and_history_routing() {
        for destination in [Destination::LoginSettings, Destination::History] {
            assert_eq!(
                Destination::from_details(&destination.details()),
                destination
            );
        }
    }

    #[test]
    fn older_failure_notifications_without_timestamp_still_open_details() {
        let original = Destination::Failure {
            name: "Old failure".into(),
            command: "false".into(),
            path: "/usr/bin:/bin".into(),
            logs: "Original logs".into(),
            started_at: None,
            failed_at: None,
        };
        assert_eq!(Destination::from_details(&original.details()), original);
    }
}
