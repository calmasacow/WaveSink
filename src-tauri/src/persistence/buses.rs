use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Node-name prefix for user-created mixes.
pub const BUS_PREFIX: &str = "sink_bus_";
pub const MAX_BUSES: usize = 8;

/// True if `name` is a mix bus node (not a channel, not a service node).
pub fn is_bus_name(name: &str) -> bool {
    name.starts_with(BUS_PREFIX)
}

/// One user-defined mix: a capturable virtual source carrying the chosen
/// channels. The label is what recorders (OBS etc.) display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusDef {
    /// Stable node name, e.g. "sink_stream" or "sink_bus_voice_only".
    pub name: String,
    /// Display label - also the device description recorders see.
    pub label: String,
    /// Bundled SVG icon id shown by the matrix header.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub icon_color: Option<String>,
    /// Exclude mode (false): channels carried by this mix.
    /// Exclude mode (true): channels kept OUT of this mix.
    pub channels: Vec<String>,
    /// True = the mix carries every channel except `channels` ("everything but
    /// music"). False = the mix carries exactly `channels` (manual selection).
    #[serde(default)]
    pub exclude: bool,
    /// Playback level recorders hear (0-100%). Persisted so a mix keeps its
    /// level across UI remounts, profile switches, and restarts.
    #[serde(default = "default_volume")]
    pub volume_percent: u8,
    /// Muted for recorders (they hear silence). Persisted like the volume.
    #[serde(default)]
    pub muted: bool,
    /// Per-member send level (0-100%; absent = 100%), independent of the
    /// member's own volume. Keyed by sink name or hardware input id.
    #[serde(default)]
    pub member_gains: HashMap<String, u8>,
}

fn default_volume() -> u8 {
    100
}

impl BusDef {
    /// The channels this mix actually carries, given the full channel set.
    pub fn effective_members(&self, all_channels: &[String]) -> Vec<String> {
        if self.exclude {
            all_channels
                .iter()
                .filter(|c| !self.channels.contains(c))
                .cloned()
                .collect()
        } else {
            self.channels.clone()
        }
    }
}

/// The user's mixes, stored at `$XDG_CONFIG_HOME/wavesink/buses.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Buses {
    pub buses: Vec<BusDef>,
}

impl Default for Buses {
    fn default() -> Self {
        Self {
            buses: vec![BusDef {
                name: "sink_bus_default".into(),
                label: "Default".into(),
                icon: Some("system".into()),
                icon_color: Some("purple".into()),
                channels: Vec::new(),
                exclude: true,
                volume_percent: 100,
                muted: false,
                member_gains: HashMap::new(),
            }],
        }
    }
}

impl Buses {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        Ok(crate::persistence::app_config_dir()?.join("buses.json"))
    }

    /// Load from disk. Old automatic Master Mix definitions are deliberately
    /// discarded: routing is now explicit and every mix is user-owned.
    pub fn load(_legacy_channels: &crate::persistence::channels::Channels) -> Self {
        let path = match Self::config_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        match fs::read_to_string(&path) {
            Ok(raw) => {
                // A present-but-broken file is a torn or hand-edited write:
                // log it rather than resetting the user's mixes in silence.
                let mut buses = match serde_json::from_str::<Self>(&raw) {
                    Ok(buses) => buses,
                    Err(e) => {
                        eprintln!("wavesink: buses.json is unreadable ({e}); using defaults");
                        Self::default()
                    }
                };
                buses
                    .buses
                    .retain(|bus| !(bus.name == "sink_stream" && bus.label == "Master Mix"));
                buses.clamp_loaded();
                buses
            }
            Err(_) => Self::default(),
        }
    }

    /// A hand-edited buses.json degrades to the documented ranges instead
    /// of riding through to the UI (the `EqConfig::clamp_ranges` rule).
    pub(crate) fn clamp_loaded(&mut self) {
        for bus in &mut self.buses {
            if bus.icon_color.is_none() {
                bus.icon_color = Some("purple".into());
            }
            bus.volume_percent = bus.volume_percent.min(crate::commands::routing::MAX_VOLUME);
            bus.member_gains.retain(|_, percent| {
                *percent = (*percent).min(crate::commands::routing::MAX_VOLUME);
                *percent != 100
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_master_is_discarded_on_load() {
        let mut buses: Buses = serde_json::from_str(
            r#"{"buses":[{"name":"sink_stream","label":"Master Mix","channels":[],"exclude":false}]}"#,
        )
        .unwrap();
        buses
            .buses
            .retain(|bus| !(bus.name == "sink_stream" && bus.label == "Master Mix"));
        assert!(buses.buses.is_empty());
    }
}
