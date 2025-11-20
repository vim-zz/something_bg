use std::path::PathBuf;

use something_bg_core::platform::AppPaths;

/// macOS implementation of application paths.
/// Keeps existing layout under ~/.config/something_bg for now.
#[derive(Default)]
pub struct MacPaths;

impl AppPaths for MacPaths {
    fn config_path(&self) -> PathBuf {
        let mut base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        base.push(".config");
        base.push("something_bg");
        base.push("config.toml");
        base
    }

    fn state_path(&self) -> PathBuf {
        let mut base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        base.push(".config");
        base.push("something_bg");
        base.push("task_state.toml");
        base
    }
}
