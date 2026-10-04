use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;

/// Per-channel output device choices from the original Sink model (JSON in
/// `wavesink/outputs.json`). Channels no longer play to devices directly -
/// mixes do - so this is read only to carry old setups forward.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelOutputs {
    pub outputs: HashMap<String, Option<String>>,
    /// Channels with auto-failover off route only to their chosen device and
    /// stay silent when it's gone; `serde(default)` keeps old configs loading.
    #[serde(default)]
    pub no_failover: HashSet<String>,
}

impl ChannelOutputs {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        Ok(crate::persistence::app_config_dir()?.join("outputs.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                eprintln!("wavesink: ignoring malformed {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize outputs: {e}")))?;
        super::write_atomic(&path, &json)?;
        Ok(())
    }

    /// Drop all state for a removed channel.
    pub fn remove(&mut self, sink_name: &str) {
        self.outputs.remove(sink_name);
        self.no_failover.remove(sink_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_configs_load() {
        // Written before the failover flag existed: no `no_failover` key.
        let legacy = r#"{"outputs":{"sink_game":"dev","sink_music":null}}"#;
        let o: ChannelOutputs = serde_json::from_str(legacy).expect("legacy loads");
        assert_eq!(o.outputs.get("sink_game"), Some(&Some("dev".to_string())));
        assert!(o.no_failover.is_empty());
    }

    #[test]
    fn remove_drops_a_channel() {
        let mut o: ChannelOutputs =
            serde_json::from_str(r#"{"outputs":{"sink_game":"dev"},"no_failover":["sink_game"]}"#)
                .unwrap();
        o.remove("sink_game");
        assert!(o.outputs.is_empty() && o.no_failover.is_empty());
    }
}
