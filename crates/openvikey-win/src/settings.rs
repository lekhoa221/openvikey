//! Versioned standalone product settings; contains no typing history.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use openvikey_core::types::{InputMethod, TonePlacement};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SETTINGS_VERSION: u32 = 1;

const fn settings_version() -> u32 {
    SETTINGS_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupMode {
    Viet,
    English,
    RestoreLast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppTransformPolicy {
    Default,
    Allow,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppLearningPolicy {
    Default,
    Allow,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppInjectProfile {
    Auto,
    Win32,
    Electron,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppPolicyV1 {
    pub executable: String,
    pub transform: AppTransformPolicy,
    pub learning: AppLearningPolicy,
    pub inject_profile: AppInjectProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeySettingsV1 {
    pub toggle_mode: String,
    pub accept_top: String,
    pub reject_top: String,
    pub undo_last: String,
    pub forget_last: String,
}

impl Default for HotkeySettingsV1 {
    fn default() -> Self {
        Self {
            toggle_mode: "LeftCtrl+LeftShift".to_owned(),
            accept_top: "Ctrl+.".to_owned(),
            reject_top: "Ctrl+,".to_owned(),
            undo_last: "Ctrl+Shift+Z".to_owned(),
            forget_last: "Ctrl+Shift+.".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SettingsV1 {
    #[serde(default = "settings_version")]
    pub version: u32,
    pub input_method: InputMethod,
    pub tone_placement: TonePlacement,
    pub mode_on_start: StartupMode,
    pub last_mode_viet: bool,
    pub show_suggestions: bool,
    #[serde(default)]
    pub allow_terminal: bool,
    pub start_with_windows: bool,
    #[serde(default)]
    pub hotkeys: HotkeySettingsV1,
    #[serde(default)]
    pub app_policies: Vec<AppPolicyV1>,
}

impl Default for SettingsV1 {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            input_method: InputMethod::Vni,
            tone_placement: TonePlacement::Modern,
            mode_on_start: StartupMode::RestoreLast,
            last_mode_viet: true,
            show_suggestions: true,
            allow_terminal: false,
            start_with_windows: false,
            hotkeys: HotkeySettingsV1::default(),
            app_policies: Vec::new(),
        }
    }
}

impl SettingsV1 {
    #[must_use]
    pub fn starts_in_vietnamese(&self) -> bool {
        match self.mode_on_start {
            StartupMode::Viet => true,
            StartupMode::English => false,
            StartupMode::RestoreLast => self.last_mode_viet,
        }
    }
}

#[derive(Debug, Error)]
pub enum SettingsLoadError {
    #[error("settings I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings JSON is invalid: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unsupported settings version {0}")]
    UnsupportedVersion(u32),
}

#[must_use]
pub fn default_settings_path(local_app_data: Option<impl AsRef<Path>>) -> PathBuf {
    let root = local_app_data.map_or_else(
        || PathBuf::from("OpenViKey"),
        |base| base.as_ref().join("OpenViKey"),
    );
    root.join("settings.json")
}

/// Missing settings return safe standalone defaults; malformed settings fail visibly.
pub fn load_settings(path: &Path) -> Result<SettingsV1, SettingsLoadError> {
    let payload = match fs::read(path) {
        Ok(payload) => payload,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SettingsV1::default());
        }
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_slice(&payload)?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .unwrap_or(SETTINGS_VERSION);
    if version != SETTINGS_VERSION {
        return Err(SettingsLoadError::UnsupportedVersion(version));
    }
    Ok(serde_json::from_value(value)?)
}

pub fn save_settings(path: &Path, settings: &SettingsV1) -> std::io::Result<()> {
    if settings.version != SETTINGS_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to save an unsupported settings version",
        ));
    }
    let payload = serde_json::to_vec_pretty(settings)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_file_name("settings.json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)?;
    file.write_all(&payload)?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(temp, path)
}

/// Atomically mutate one validated settings file without replacing unknown/corrupt payloads.
pub fn mutate_settings(
    path: &Path,
    mutate: impl FnOnce(&mut SettingsV1),
) -> Result<SettingsV1, SettingsLoadError> {
    let mut settings = load_settings(path)?;
    mutate(&mut settings);
    save_settings(path, &settings)?;
    Ok(settings)
}
