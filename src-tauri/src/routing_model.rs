//! Wave Link-style routing contract.
//!
//! This is deliberately independent from the PipeWire implementation.  The
//! UI and control-surface commands speak in terms of inputs, mixes and cells;
//! the audio backend is responsible for materialising those objects as
//! PipeWire nodes and links.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::buses::{Buses, MixRole};
use crate::persistence::channels::Channels;
use crate::persistence::outputs::ChannelOutputs;

pub const ROUTING_VERSION: u32 = 1;
pub const ROUTING_FILE: &str = "routing.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Software,
    Hardware,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FxChain {
    #[serde(default)]
    pub high_pass_hz: Option<u32>,
    #[serde(default)]
    pub eq_enabled: bool,
    #[serde(default)]
    pub gate_enabled: bool,
    #[serde(default)]
    pub compressor_enabled: bool,
    #[serde(default)]
    pub limiter_enabled: bool,
    #[serde(default = "default_gate_threshold")]
    pub gate_threshold_db: f32,
    #[serde(default = "default_compressor_threshold")]
    pub compressor_threshold_db: f32,
    #[serde(default = "default_compressor_ratio")]
    pub compressor_ratio: f32,
    #[serde(default = "default_limiter_ceiling")]
    pub limiter_ceiling_db: f32,
}

impl Default for FxChain {
    fn default() -> Self {
        Self {
            high_pass_hz: None,
            eq_enabled: false,
            gate_enabled: false,
            compressor_enabled: false,
            limiter_enabled: false,
            gate_threshold_db: default_gate_threshold(),
            compressor_threshold_db: default_compressor_threshold(),
            compressor_ratio: default_compressor_ratio(),
            limiter_ceiling_db: default_limiter_ceiling(),
        }
    }
}

impl FxChain {
    /// Whether any processing stage is on. With none on, the input links
    /// straight into its mixes and no DSP runs at all.
    pub fn is_active(&self) -> bool {
        self.gate_enabled || self.compressor_enabled || self.limiter_enabled
    }

    /// The same values clamped to DSP-safe ranges, non-finite replaced by
    /// defaults, so a malformed or hostile payload can't destabilize the DSP.
    pub fn clamped(&self) -> Self {
        let finite = |v: f32, fallback: f32, lo: f32, hi: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                fallback
            }
        };
        Self {
            gate_threshold_db: finite(
                self.gate_threshold_db,
                default_gate_threshold(),
                -100.0,
                0.0,
            ),
            compressor_threshold_db: finite(
                self.compressor_threshold_db,
                default_compressor_threshold(),
                -100.0,
                0.0,
            ),
            compressor_ratio: finite(self.compressor_ratio, default_compressor_ratio(), 1.0, 20.0),
            limiter_ceiling_db: finite(
                self.limiter_ceiling_db,
                default_limiter_ceiling(),
                -60.0,
                0.0,
            ),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputDef {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// Stable bundled-icon background token, not an arbitrary CSS value.
    #[serde(default)]
    pub icon_color: Option<String>,
    pub kind: InputKind,
    /// PipeWire node/source name, or the stable software-channel name.
    pub source_name: String,
    #[serde(default = "default_level")]
    pub volume_percent: u8,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub fx: FxChain,
    pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputBinding {
    pub device: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixDef {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub icon_color: Option<String>,
    #[serde(default = "default_level")]
    pub volume_percent: u8,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub output_bindings: Vec<OutputBinding>,
    pub order: u32,
    #[serde(default)]
    pub role: MixRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCell {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_level")]
    pub send_percent: u8,
    #[serde(default)]
    pub muted: bool,
}

impl Default for RouteCell {
    fn default() -> Self {
        Self {
            enabled: false,
            send_percent: 100,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingModel {
    pub version: u32,
    pub inputs: Vec<InputDef>,
    pub mixes: Vec<MixDef>,
    /// input id -> mix id -> cell
    #[serde(default)]
    pub routes: BTreeMap<String, BTreeMap<String, RouteCell>>,
    #[serde(default)]
    pub monitor_mix: Option<String>,
    #[serde(default)]
    pub hidden_devices: Vec<String>,
}

impl Default for RoutingModel {
    fn default() -> Self {
        Self {
            version: ROUTING_VERSION,
            inputs: Vec::new(),
            mixes: Vec::new(),
            routes: BTreeMap::new(),
            monitor_mix: None,
            hidden_devices: Vec::new(),
        }
    }
}

fn default_level() -> u8 {
    100
}

fn default_gate_threshold() -> f32 {
    -40.0
}
fn default_compressor_threshold() -> f32 {
    -18.0
}
fn default_compressor_ratio() -> f32 {
    3.0
}
fn default_limiter_ceiling() -> f32 {
    -1.0
}

#[cfg(test)]
mod fx_tests {
    use super::FxChain;

    #[test]
    fn fx_defaults_are_safe_for_legacy_routing_files() {
        let fx: FxChain = serde_json::from_str("{}").expect("legacy FX");
        assert_eq!(fx.gate_threshold_db, -40.0);
        assert_eq!(fx.compressor_threshold_db, -18.0);
        assert_eq!(fx.compressor_ratio, 3.0);
        assert_eq!(fx.limiter_ceiling_db, -1.0);
    }
}
fn default_true() -> bool {
    true
}

impl RoutingModel {
    pub fn config_path() -> Result<PathBuf, SinkError> {
        Ok(crate::persistence::app_config_dir()?.join(ROUTING_FILE))
    }

    /// Migrate the old channel/bus topology once, keeping the old JSON files
    /// intact and writing a timestamped backup beside the new contract.
    pub fn load_or_migrate(channels: &Channels, buses: &Buses, outputs: &ChannelOutputs) -> Self {
        let path = Self::config_path().ok();
        if let Some(path) = &path {
            if let Ok(raw) = fs::read_to_string(path) {
                if let Ok(mut model) = serde_json::from_str::<Self>(&raw) {
                    model.clamp_levels();
                    return model;
                }
            }
        }
        let model = Self::from_legacy(channels, buses, outputs);
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = crate::persistence::ensure_private_dir(parent);
                let backup = parent.join(format!(
                    "routing-migration-backup-{}",
                    crate::persistence::unix_now()
                ));
                let _ = fs::create_dir_all(&backup);
                for name in [
                    "channels.json",
                    "buses.json",
                    "outputs.json",
                    "mic.json",
                    "eq.json",
                ] {
                    let old = parent.join(name);
                    if old.exists() {
                        let _ = fs::copy(&old, backup.join(name));
                    }
                }
            }
            let _ = model.save();
        }
        model
    }

    /// Levels saved under the old 150% ceiling come back at unity: the
    /// pipeline never amplifies.
    pub fn clamp_levels(&mut self) {
        let max = crate::commands::routing::MAX_VOLUME;
        for input in &mut self.inputs {
            input.volume_percent = input.volume_percent.min(max);
        }
        for mix in &mut self.mixes {
            mix.volume_percent = mix.volume_percent.min(max);
        }
        for cells in self.routes.values_mut() {
            for cell in cells.values_mut() {
                cell.send_percent = cell.send_percent.min(max);
            }
        }
    }

    pub fn from_legacy(channels: &Channels, buses: &Buses, outputs: &ChannelOutputs) -> Self {
        let inputs = channels
            .channels
            .iter()
            .enumerate()
            .map(|(order, c)| InputDef {
                id: c.name.clone(),
                label: c.label.clone(),
                icon: c.icon.clone(),
                icon_color: c.icon_color.clone(),
                kind: InputKind::Software,
                source_name: c.name.clone(),
                volume_percent: c.volume_percent,
                muted: c.muted,
                fx: FxChain::default(),
                order: order as u32,
            })
            .collect::<Vec<_>>();
        let mixes = buses
            .buses
            .iter()
            .enumerate()
            .map(|(order, b)| MixDef {
                id: b.name.clone(),
                label: b.label.clone(),
                icon: b.icon.clone(),
                icon_color: b.icon_color.clone(),
                volume_percent: b.volume_percent,
                muted: b.muted,
                output_bindings: if b.name == "sink_stream" {
                    outputs_for_legacy(outputs)
                } else {
                    Vec::new()
                },
                order: order as u32,
                role: b.role,
            })
            .collect::<Vec<_>>();
        let mut routes = BTreeMap::new();
        for input in &inputs {
            let mut cells = BTreeMap::new();
            for mix in buses.buses.iter() {
                let enabled = mix
                    .effective_members(&inputs.iter().map(|i| i.id.clone()).collect::<Vec<_>>())
                    .contains(&input.id);
                let send = mix.member_gains.get(&input.id).copied().unwrap_or(100);
                cells.insert(
                    mix.name.clone(),
                    RouteCell {
                        enabled,
                        send_percent: send,
                        muted: false,
                    },
                );
            }
            routes.insert(input.id.clone(), cells);
        }
        Self {
            version: ROUTING_VERSION,
            inputs,
            mixes,
            routes,
            monitor_mix: None,
            hidden_devices: Vec::new(),
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            crate::persistence::ensure_private_dir(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SinkError::Config(format!("serialize routing: {e}")))?;
        crate::persistence::write_atomic(&path, json)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn cell(&self, input: &str, mix: &str) -> RouteCell {
        self.routes
            .get(input)
            .and_then(|m| m.get(mix))
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_cell(&mut self, input: &str, mix: &str, cell: RouteCell) -> Result<(), SinkError> {
        if !self.inputs.iter().any(|i| i.id == input) {
            return Err(SinkError::UnknownSink(input.into()));
        }
        if !self.mixes.iter().any(|m| m.id == mix) {
            return Err(SinkError::UnknownSink(mix.into()));
        }
        self.routes.entry(input.into()).or_default().insert(
            mix.into(),
            RouteCell {
                send_percent: cell.send_percent.min(crate::commands::routing::MAX_VOLUME),
                ..cell
            },
        );
        Ok(())
    }
}

fn outputs_for_legacy(outputs: &ChannelOutputs) -> Vec<OutputBinding> {
    let mut seen = std::collections::BTreeSet::new();
    outputs
        .outputs
        .values()
        .filter_map(|o| o.clone())
        .filter(|o| seen.insert(o.clone()))
        .map(|device| OutputBinding {
            device,
            enabled: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn fx_is_active_only_with_a_stage_on() {
        assert!(!FxChain::default().is_active());
        for fx in [
            FxChain {
                gate_enabled: true,
                ..FxChain::default()
            },
            FxChain {
                compressor_enabled: true,
                ..FxChain::default()
            },
            FxChain {
                limiter_enabled: true,
                ..FxChain::default()
            },
        ] {
            assert!(fx.is_active());
        }
    }

    #[test]
    fn fx_clamps_hostile_values_and_keeps_sane_ones() {
        let hostile = FxChain {
            gate_threshold_db: f32::NAN,
            compressor_threshold_db: 40.0,
            compressor_ratio: -3.0,
            limiter_ceiling_db: f32::NEG_INFINITY,
            ..FxChain::default()
        }
        .clamped();
        assert_eq!(hostile.gate_threshold_db, -40.0);
        assert_eq!(hostile.compressor_threshold_db, 0.0);
        assert_eq!(hostile.compressor_ratio, 1.0);
        assert_eq!(hostile.limiter_ceiling_db, -1.0);
        let sane = FxChain {
            gate_enabled: true,
            ..FxChain::default()
        };
        assert_eq!(sane.clamped(), sane);
    }
    use super::*;

    #[test]
    fn cells_are_independent_from_each_other() {
        let channels = Channels::default();
        let mut buses = Buses::default();
        let bus = buses.add("Stream").unwrap();
        buses
            .set_members(
                &bus.name,
                channels.channels.iter().map(|c| c.name.clone()).collect(),
            )
            .unwrap();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        model
            .set_cell(
                "sink_game",
                &bus.name,
                RouteCell {
                    enabled: true,
                    send_percent: 70,
                    muted: false,
                },
            )
            .unwrap();
        assert_eq!(model.cell("sink_game", &bus.name).send_percent, 70);
        assert_eq!(
            model.cell("sink_chat", &bus.name),
            RouteCell {
                enabled: false,
                send_percent: 100,
                muted: false
            }
        );
    }

    #[test]
    fn empty_mix_set_stays_empty() {
        let model = RoutingModel::from_legacy(
            &Channels::default(),
            &Buses { buses: Vec::new() },
            &ChannelOutputs::default(),
        );
        assert!(model.mixes.is_empty());
    }
}
