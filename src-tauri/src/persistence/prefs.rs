use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// What the meters do while the window is open but not focused (WaveSink
/// on a second monitor while gaming, say).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MeterUnfocused {
    /// Drop to 10 fps.
    #[default]
    Reduced,
    /// Keep the full rate.
    Full,
    /// Stop metering until focused again.
    Off,
}

/// The meter frame rates offered in Settings.
pub const METER_FPS_CHOICES: [u8; 3] = [10, 20, 30];
/// The rate a reduced (unfocused) meter runs at.
pub const METER_REDUCED_FPS: u8 = 10;

/// App preferences, stored at `$XDG_CONFIG_HOME/wavesink/prefs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
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
    /// Pro Audio Metering: dBFS meter scale, standard color zones and dB
    /// labels. Off = the familiar 0-100% everywhere.
    #[serde(default)]
    pub meter_pro: bool,
    /// Meter frame rate while the window is focused (10, 20 or 30).
    #[serde(default = "default_meter_fps")]
    pub meter_fps: u8,
    #[serde(default)]
    pub meter_unfocused: MeterUnfocused,
}

fn default_true() -> bool {
    true
}

fn default_meter_fps() -> u8 {
    30
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            onboarded: false,
            balance_a: None,
            balance_b: None,
            show_balance: true,
            start_minimized: false,
            meter_pro: false,
            meter_fps: default_meter_fps(),
            meter_unfocused: MeterUnfocused::default(),
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
    fn decorate_marks_wavesink_nodes() {
        assert_eq!(Prefs::default().decorate("Game"), "Game (WaveSink)");
    }

    #[test]
    fn malformed_prefs_degrade_to_defaults() {
        // Corrupt / partially-written files must never panic or block
        // launch - they fall back to defaults.
        assert_eq!(Prefs::parse(""), Prefs::default());
        assert_eq!(Prefs::parse("{not json"), Prefs::default());
        assert_eq!(Prefs::parse("[]"), Prefs::default());
        assert_eq!(
            Prefs::parse(r#"{"start_minimized":"not a bool"}"#),
            Prefs::default()
        );
        // Unknown fields are tolerated (including the retired
        // device_label_style); known fields still apply.
        let p = Prefs::parse(r#"{"device_label_style":"suffix","onboarded":true}"#);
        assert!(p.onboarded);
    }

    #[test]
    fn meter_prefs_default_for_older_files() {
        // A prefs.json written before meter settings existed.
        let p = Prefs::parse(r#"{"onboarded":true}"#);
        assert!(!p.meter_pro);
        assert_eq!(p.meter_fps, 30);
        assert_eq!(p.meter_unfocused, MeterUnfocused::Reduced);
        let p = Prefs::parse(r#"{"meter_pro":true,"meter_fps":20,"meter_unfocused":"off"}"#);
        assert!(p.meter_pro);
        assert_eq!(p.meter_fps, 20);
        assert_eq!(p.meter_unfocused, MeterUnfocused::Off);
    }
}
