//! Hotkey settings: key bindings for the X11 fallback.
//! Under the desktop portal the bindings live with the desktop, not here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Action id -> accelerator, X11 only.
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
}

impl HotkeyConfig {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        Ok(super::app_config_dir()?.join("hotkeys.json"))
    }

    pub fn load() -> Self {
        Self::config_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            super::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize hotkeys: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }
}
