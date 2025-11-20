pub mod config;
pub mod scheduler;
pub mod tunnel;

/// Interfaces that platform shells can implement to adapt the core library
/// without pulling in platform-specific dependencies.
pub mod platform {
    /// Trait for tray/menu UI surfaces.
    pub trait TrayUi {
        type MenuId;
        fn refresh(&self);
        fn set_active(&self, any_active: bool);
        fn set_menu_item_state(&self, id: &Self::MenuId, active: bool);
    }

    /// Trait for dispatching user-visible notifications.
    pub trait Notifier {
        fn info(&self, title: &str, body: &str);
        fn warn(&self, title: &str, body: &str);
        fn error(&self, title: &str, body: &str);
    }

    /// Trait for logging sinks beyond the default logger.
    pub trait LoggerSink {
        fn init(&self);
    }

    /// Trait for platform-correct config/cache paths.
    pub trait AppPaths {
        fn config_path(&self) -> std::path::PathBuf;
        fn state_path(&self) -> std::path::PathBuf;
    }

    /// Trait for spawning and stopping processes; allows platform-specific policies.
    pub trait ProcessSpawner {
        fn spawn(
            &self,
            program: &str,
            args: &[String],
            path: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
        fn kill(
            &self,
            program: &str,
            args: &[String],
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    }
}
