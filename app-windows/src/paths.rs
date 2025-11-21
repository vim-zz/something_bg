use std::path::PathBuf;

use something_bg_core::platform::AppPaths;

#[derive(Default)]
pub struct WindowsPaths;

impl AppPaths for WindowsPaths {
    fn config_path(&self) -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("something_bg")
            .join("config.toml")
    }

    fn state_path(&self) -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("something_bg")
            .join("task_state.toml")
    }
}
