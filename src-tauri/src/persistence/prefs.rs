use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Legacy device-label preference retained for config compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLabelStyle {
    /// "Game"
    #[default]
    Plain,
    /// "Game (WaveSink)"
    Suffix,
    /// "WaveSink · Game"
    Prefix,
}

/// App preferences, stored at `$XDG_CONFIG_HOME/wavesink/prefs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub device_label_style: DeviceLabelStyle,
    /// First-run tutorial completed (false = show it on launch).
    #[serde(default)]
    pub onboarded: bool,
    /// ChatMix-style balance: the two channel sink names being balanced
    /// (None = auto: Game/Chat when present, else the first two channels).
    #[serde(default)]
    pub balance_a: Option<String>,
    #[serde(default)]
    pub balance_b: Option<String>,
    /// Show the balance slider in the title bar.
    #[serde(default = "default_true")]
    pub show_balance: bool,
    /// When autostarting on login, boot straight to the tray instead of
    /// showing the window (only meaningful with autostart enabled).
    #[serde(default)]
    pub start_minimized: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            device_label_style: DeviceLabelStyle::default(),
            onboarded: false,
            balance_a: None,
            balance_b: None,
            show_balance: true,
            start_minimized: false,
        }
    }
}

impl Prefs {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        Ok(crate::persistence::app_config_dir()?.join("prefs.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        fs::read_to_string(&path)
            .map(|raw| Self::parse(&raw))
            .unwrap_or_default()
    }

    /// Parse stored prefs; malformed input degrades to defaults rather
    /// than blocking launch.
    fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_else(|e| {
            eprintln!("wavesink: ignoring malformed prefs: {e}");
            Self::default()
        })
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize prefs: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    /// Every WaveSink-owned PipeWire node carries this suffix so system device
    /// pickers clearly distinguish virtual channels from physical hardware.
    pub fn decorate(&self, label: &str) -> String {
        format!("{label} (WaveSink)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorate_styles() {
        let mut p = Prefs::default();
        assert_eq!(p.decorate("Game"), "Game (WaveSink)");
        p.device_label_style = DeviceLabelStyle::Suffix;
        assert_eq!(p.decorate("Game"), "Game (WaveSink)");
        p.device_label_style = DeviceLabelStyle::Prefix;
        assert_eq!(p.decorate("Game"), "Game (WaveSink)");
    }

    #[test]
    fn malformed_prefs_degrade_to_defaults() {
        // Corrupt / partially-written files must never panic or block
        // launch - they fall back to defaults.
        assert_eq!(Prefs::parse(""), Prefs::default());
        assert_eq!(Prefs::parse("{not json"), Prefs::default());
        assert_eq!(Prefs::parse("[]"), Prefs::default());
        assert_eq!(
            Prefs::parse(r#"{"device_label_style":"bogus_style"}"#),
            Prefs::default()
        );
        // Unknown fields are tolerated; known fields still apply.
        let p = Prefs::parse(r#"{"device_label_style":"suffix","future_field":1}"#);
        assert_eq!(p.device_label_style, DeviceLabelStyle::Suffix);
    }
}
