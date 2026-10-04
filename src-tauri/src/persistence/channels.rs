use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Sink node names reserved by WaveSink itself (not user channels).
pub const RESERVED_SINK_NAMES: [&str; 2] = ["sink_mic", "sink_stream"];
/// Upper bound on user channels (level-meter slots are budgeted for this).
pub const MAX_CHANNELS: usize = 10;

/// One user-defined mixer channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelDef {
    /// PipeWire sink node name, e.g. "sink_game". Stable once created.
    pub name: String,
    /// Display label, e.g. "Game". Renameable.
    pub label: String,
    /// Material Symbol name for the strip icon (None = legacy default).
    #[serde(default)]
    pub icon: Option<String>,
    /// Stable palette token used behind the bundled SVG icon.
    #[serde(default)]
    pub icon_color: Option<String>,
    /// Fader position (0-100%). Persisted so a channel keeps its level
    /// across restarts even when no profile is active.
    #[serde(default = "default_volume")]
    pub volume_percent: u8,
    /// Muted state, persisted alongside the volume.
    #[serde(default)]
    pub muted: bool,
}

fn default_volume() -> u8 {
    100
}

/// The original Sink channel set (`$XDG_CONFIG_HOME/wavesink/channels.json`).
/// Read only to migrate a setup into the routing model, and as the classic
/// four channels a fresh install or blank profile starts from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channels {
    pub channels: Vec<ChannelDef>,
}

impl Default for Channels {
    fn default() -> Self {
        let def = |name: &str, label: &str, icon: &str| ChannelDef {
            name: name.to_string(),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            icon_color: Some("blue".to_string()),
            volume_percent: default_volume(),
            muted: false,
        };
        Self {
            channels: vec![
                def("sink_game", "Game", "sports_esports"),
                def("sink_chat", "Chat", "forum"),
                def("sink_music", "Music", "music_note"),
                def("sink_system", "System", "desktop_windows"),
            ],
        }
    }
}

impl Channels {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        Ok(crate::persistence::app_config_dir()?.join("channels.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        // A missing file is first run (silent default); a present-but-broken
        // one is a torn or hand-edited write we log rather than honour.
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => return Self::default(),
        };
        match Self::parse(&raw) {
            Some(c) if !c.channels.is_empty() => c,
            Some(_) => {
                eprintln!("wavesink: channels.json held no valid channels; using defaults");
                Self::default()
            }
            None => {
                eprintln!("wavesink: channels.json is unreadable (corrupt?); using defaults");
                Self::default()
            }
        }
    }

    /// Sanitize the channel set (drops reserved names, missing `sink_`
    /// prefix, duplicates); `None` means invalid JSON, not merely empty.
    fn parse(raw: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(raw).ok()?;
        let mut seen = std::collections::HashSet::new();
        let mut channels = Vec::new();
        for def in parsed.channels {
            let name = def.name.as_str();
            let valid = name.starts_with("sink_")
                && !RESERVED_SINK_NAMES.contains(&name)
                && !crate::persistence::buses::is_bus_name(name)
                && seen.insert(def.name.clone());
            if valid && channels.len() < MAX_CHANNELS {
                let mut def = def;
                def.volume_percent = def.volume_percent.min(crate::commands::routing::MAX_VOLUME);
                channels.push(def);
            } else {
                eprintln!(
                    "wavesink: dropping invalid channel '{}' from channels.json",
                    def.name
                );
            }
        }
        Some(Self { channels })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_classic_four() {
        let c = Channels::default();
        assert_eq!(c.channels.len(), 4);
        assert_eq!(c.channels[0].name, "sink_game");
    }

    #[test]
    fn a_bus_named_channel_is_dropped_on_load() {
        // A hand-edited channels.json can't smuggle a mix-namespace name in.
        let raw = r#"{"channels":[
            {"name":"sink_bus_evil","label":"Evil"},
            {"name":"sink_game","label":"Game"}
        ]}"#;
        let parsed = Channels::parse(raw).expect("parses");
        assert_eq!(parsed.channels.len(), 1);
        assert_eq!(parsed.channels[0].name, "sink_game");
    }

    #[test]
    fn parse_keeps_valid_and_fills_serde_defaults() {
        // Bug shape: old-shape entries (no icon, or the retired stream_mix
        // flag) must still load via serde defaults.
        let raw = r#"{"channels":[
            {"name":"sink_game","label":"Game"},
            {"name":"sink_music","label":"Music","icon":"music_note","stream_mix":false}
        ]}"#;
        let c = Channels::parse(raw).expect("valid json");
        assert_eq!(c.channels.len(), 2);
        assert_eq!(c.channels[0].icon, None);
    }

    #[test]
    fn parse_drops_reserved_unprefixed_and_duplicate_names() {
        let raw = r#"{"channels":[
            {"name":"sink_game","label":"Game"},
            {"name":"sink_game","label":"Dup"},
            {"name":"sink_mic","label":"Reserved"},
            {"name":"nope","label":"NoPrefix"},
            {"name":"sink_ok","label":"Fine"}
        ]}"#;
        let names: Vec<String> = Channels::parse(raw)
            .expect("valid json")
            .channels
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, ["sink_game", "sink_ok"]);
    }

    #[test]
    fn parse_caps_at_max_channels() {
        let mut items = Vec::new();
        for i in 0..(MAX_CHANNELS + 5) {
            items.push(format!(r#"{{"name":"sink_c{i}","label":"C{i}"}}"#));
        }
        let raw = format!(r#"{{"channels":[{}]}}"#, items.join(","));
        assert_eq!(
            Channels::parse(&raw).expect("valid json").channels.len(),
            MAX_CHANNELS
        );
    }

    #[test]
    fn parse_rejects_corrupt_or_empty_text() {
        assert!(Channels::parse("{ truncated").is_none());
        assert!(Channels::parse("").is_none());
    }

    #[test]
    fn parse_all_invalid_yields_empty_set() {
        // load() turns this into defaults; parse itself reports the empty set
        // so load can distinguish it from a corrupt (None) file.
        let raw = r#"{"channels":[{"name":"sink_mic","label":"x"},{"name":"bad","label":"y"}]}"#;
        assert!(Channels::parse(raw)
            .expect("valid json")
            .channels
            .is_empty());
    }
}
