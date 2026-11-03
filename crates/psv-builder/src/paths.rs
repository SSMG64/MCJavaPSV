use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "MCJavaPSV";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
}

impl AppPaths {
    /// `MCJAVAPSV_HOME` overrides the per-user data folder, which keeps tests and
    /// portable setups away from real settings.
    pub fn detect() -> Self {
        let data_dir = std::env::var_os("MCJAVAPSV_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|d| d.join(APP_DIR_NAME)))
            .unwrap_or_else(|| PathBuf::from(APP_DIR_NAME));
        Self { data_dir }
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn work_dir(&self) -> PathBuf {
        self.data_dir.join("work")
    }
}
