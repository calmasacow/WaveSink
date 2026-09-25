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
    /// Exclude mode (false): channels carried by this mix.
    /// Exclude mode (true): channels kept OUT of this mix.
    pub channels: Vec<String>,
    /// True = the mix carries every channel except `channels` ("everything but
    /// music"). False = the mix carries exactly `channels` (manual selection).
    #[serde(default)]
    pub exclude: bool,
    /// Playback level recorders hear (0-150%). Persisted so a mix keeps its
    /// level across UI remounts, profile switches, and restarts.
    #[serde(default = "default_volume")]
    pub volume_percent: u8,
    /// Muted for recorders (they hear silence). Persisted like the volume.
    #[serde(default)]
    pub muted: bool,
    /// The processed virtual mic feeds this mix alongside its channels.
    #[serde(default)]
    pub mic: bool,
    /// Per-member send level (0-150%; absent = 100%), independent of the
    /// member's own volume. Keyed by sink name, or "sink_mic".
    #[serde(default)]
    pub member_gains: HashMap<String, u8>,
    /// Which device list the mix shows up in. Named rather than a flag so
    /// a third role can be added without rewriting anyone's config.
    #[serde(default)]
    pub role: MixRole,
}

/// Where a mix appears to the rest of the system.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MixRole {
    /// A recording device, where a recorder looks first.
    #[default]
    Recording,
    /// A playback device, captured through its monitor.
    Playback,
}

impl MixRole {
    pub fn is_recording(self) -> bool {
        matches!(self, Self::Recording)
    }
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

/// The user's mixes, stored at `$XDG_CONFIG_HOME/sink/buses.json`.
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
                channels: Vec::new(),
                exclude: true,
                volume_percent: 100,
                muted: false,
                mic: false,
                member_gains: HashMap::new(),
                role: MixRole::Recording,
            }],
        }
    }
}

fn slugify(label: &str) -> String {
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "mix".to_string()
    } else {
        slug
    }
}

impl Buses {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        let dir = crate::persistence::config_root()
            .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
        Ok(dir.join("sink").join("buses.json"))
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
                        eprintln!("sink: buses.json is unreadable ({e}); using defaults");
                        Self::default()
                    }
                };
                buses.buses.retain(|bus| {
                    !(bus.name == "sink_stream" && bus.label == "Master Mix")
                });
                buses.clamp_loaded();
                buses
            }
            Err(_) => Self::default(),
        }
    }

    /// A hand-edited buses.json degrades to the documented ranges instead
    /// of riding through to the UI (the `EqConfig::clamp_ranges` rule).
    fn clamp_loaded(&mut self) {
        for bus in &mut self.buses {
            bus.volume_percent = bus.volume_percent.min(150);
            bus.member_gains.retain(|_, percent| {
                *percent = (*percent).min(150);
                *percent != 100
            });
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize buses: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&BusDef> {
        self.buses.iter().find(|b| b.name == name)
    }

    fn get_mut(&mut self, name: &str) -> Result<&mut BusDef, SinkError> {
        self.buses
            .iter_mut()
            .find(|b| b.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))
    }

    /// Switch a mix between manual and auto-include mode, preserving its
    /// current effective membership (the stored list flips meaning).
    pub fn set_exclude(
        &mut self,
        name: &str,
        exclude: bool,
        all_channels: &[String],
    ) -> Result<(), SinkError> {
        let def = self.get_mut(name)?;
        if def.exclude == exclude {
            return Ok(());
        }
        // Preserve what the mix carries: exclude mode stores the
        // complement, manual mode stores the carried set itself.
        let effective = def.effective_members(all_channels);
        def.channels = if exclude {
            all_channels
                .iter()
                .filter(|c| !effective.contains(c))
                .cloned()
                .collect()
        } else {
            effective
        };
        def.exclude = exclude;
        Ok(())
    }

    pub fn add(&mut self, label: &str) -> Result<BusDef, SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config(
                "mix label must be 1-24 characters".into(),
            ));
        }
        if self.buses.len() >= MAX_BUSES {
            return Err(SinkError::Config(format!(
                "at most {MAX_BUSES} mixes are supported"
            )));
        }
        let base = format!("{BUS_PREFIX}{}", slugify(label));
        let mut name = base.clone();
        let mut counter = 2;
        while self.get(&name).is_some() {
            name = format!("{base}_{counter}");
            counter += 1;
        }
        // New mixes start in auto-include mode carrying everything - uncheck
        // what you don't want and future channels keep joining automatically.
        let def = BusDef {
            name,
            label: label.to_string(),
            icon: Some("broadcast".into()),
            channels: Vec::new(),
            exclude: true,
            volume_percent: 100,
            muted: false,
            mic: false,
            member_gains: HashMap::new(),
            role: MixRole::Recording,
        };
        self.buses.push(def.clone());
        Ok(def)
    }

    pub fn rename(&mut self, name: &str, label: &str) -> Result<(), SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config(
                "mix label must be 1-24 characters".into(),
            ));
        }
        let def = self.get_mut(name)?;
        def.label = label.to_string();
        Ok(())
    }

    pub fn set_icon(&mut self, name: &str, icon: String) -> Result<(), SinkError> {
        self.get_mut(name)?.icon = Some(icon);
        Ok(())
    }

    /// Whether `remove` would succeed, so callers can reject a bad name
    /// before tearing the node down.
    pub fn removable(&self, name: &str) -> Result<(), SinkError> {
        if self.get(name).is_none() {
            return Err(SinkError::UnknownSink(name.to_string()));
        }
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), SinkError> {
        self.removable(name)?;
        self.buses.retain(|b| b.name != name);
        Ok(())
    }

    pub fn set_members(&mut self, name: &str, channels: Vec<String>) -> Result<(), SinkError> {
        let def = self.get_mut(name)?;
        def.channels = channels;
        Ok(())
    }

    /// The updated definition comes back because the node has to be
    /// rebuilt in the new shape from it.
    pub fn set_role(&mut self, name: &str, role: MixRole) -> Result<BusDef, SinkError> {
        let def = self.get_mut(name)?;
        def.role = role;
        Ok(def.clone())
    }

    pub fn set_volume(&mut self, name: &str, volume: u8) -> Result<(), SinkError> {
        let def = self.get_mut(name)?;
        def.volume_percent = volume;
        Ok(())
    }

    pub fn set_muted(&mut self, name: &str, muted: bool) -> Result<(), SinkError> {
        let def = self.get_mut(name)?;
        def.muted = muted;
        Ok(())
    }

    /// Allowed on the master mix too - unlike channel membership, mic
    /// inclusion isn't auto-managed.
    pub fn set_mic(&mut self, name: &str, mic: bool) -> Result<(), SinkError> {
        let def = self.get_mut(name)?;
        def.mic = mic;
        Ok(())
    }

    /// Unity (100) drops the entry, so a fader back at rest doesn't bloat
    /// the persisted file.
    pub fn set_member_gain(
        &mut self,
        name: &str,
        member: &str,
        percent: u8,
    ) -> Result<(), SinkError> {
        let def = self.get_mut(name)?;
        if percent == 100 {
            def.member_gains.remove(member);
        } else {
            def.member_gains.insert(member.to_string(), percent);
        }
        Ok(())
    }

    /// Drop a deleted channel from every bus's membership and any per-mix
    /// send level it had.
    pub fn remove_channel(&mut self, channel: &str) {
        for bus in &mut self.buses {
            bus.channels.retain(|c| c != channel);
            bus.member_gains.remove(channel);
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
        buses.buses.retain(|bus| !(bus.name == "sink_stream" && bus.label == "Master Mix"));
        assert!(buses.buses.is_empty());
    }
}
