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
use crate::persistence::buses::Buses;
use crate::persistence::channels::Channels;
use crate::persistence::outputs::ChannelOutputs;

/// 1: the legacy channels/buses files still drove the graph and this file
/// was a projection of them. 2: this model is the only source of truth.
pub const ROUTING_VERSION: u32 = 2;
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

/// The output binding that follows the desktop's default output device.
pub const SYSTEM_DEFAULT_OUTPUT: &str = "@default";

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

/// One input heard on its own: every other input is muted until it is
/// un-soloed, which puts back the mutes taken here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Solo {
    pub input: String,
    /// Each input's mute before the solo began.
    pub restore: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingModel {
    pub version: u32,
    pub inputs: Vec<InputDef>,
    pub mixes: Vec<MixDef>,
    /// input id -> mix id -> cell
    #[serde(default)]
    pub routes: BTreeMap<String, BTreeMap<String, RouteCell>>,
    /// Persisted so a restart mid-solo can still be un-soloed.
    #[serde(default)]
    pub solo: Option<Solo>,
}

impl Default for RoutingModel {
    fn default() -> Self {
        Self {
            version: ROUTING_VERSION,
            inputs: Vec::new(),
            mixes: Vec::new(),
            routes: BTreeMap::new(),
            solo: None,
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

    /// Load the one source of truth for inputs, mixes and routes. Older
    /// setups migrate once from the original channels/buses/outputs files,
    /// which are left in place (and backed up) but never written again:
    /// - no routing.json: the model is built from those files;
    /// - a version-1 routing.json: those files still owned labels, icons,
    ///   levels and mutes, so they are folded in; the cells keep what the
    ///   matrix showed.
    pub fn load_or_migrate() -> Self {
        let legacy = || {
            let channels = Channels::load();
            let buses = Buses::load(&channels);
            let outputs = ChannelOutputs::load();
            Self::from_legacy(&channels, &buses, &outputs)
        };
        let path = Self::config_path().ok();
        let saved = path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str::<Self>(&raw).ok());
        let mut model = match saved {
            Some(model) if model.version >= ROUTING_VERSION => {
                let mut model = model;
                model.clamp_levels();
                return model;
            }
            Some(mut model) => {
                model.fold_legacy(&legacy());
                model
            }
            None => legacy(),
        };
        model.version = ROUTING_VERSION;
        model.clamp_levels();
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
                    "routing.json",
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

    /// Channels reach outputs only through mixes. A setup that relied on
    /// channels playing straight to the default device would go silent, so
    /// when no mix plays anywhere, the first mix follows the system default.
    /// Returns whether it changed anything.
    pub fn ensure_an_output(&mut self) -> bool {
        let plays_somewhere = self
            .mixes
            .iter()
            .any(|mix| mix.output_bindings.iter().any(|b| b.enabled));
        // The list order is the matrix's column order: leftmost mix.
        let Some(first) = self.mixes.first_mut() else {
            return false;
        };
        if plays_somewhere {
            return false;
        }
        first.output_bindings.push(OutputBinding {
            device: SYSTEM_DEFAULT_OUTPUT.to_string(),
            enabled: true,
        });
        true
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
            solo: None,
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

/// A label as a node-name slug: lowercase ASCII words joined by `_`.
fn slugify(label: &str, fallback: &str) -> String {
    let slug = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    }
}

fn valid_label(label: &str, what: &str) -> Result<String, SinkError> {
    let label = label.trim();
    if label.is_empty() || label.len() > 24 {
        return Err(SinkError::Config(format!(
            "{what} label must be 1-24 characters"
        )));
    }
    Ok(label.to_string())
}

impl RoutingModel {
    /// Fold the original files' view into a version-1 model (see
    /// `load_or_migrate`): labels, icons, levels and mutes come from them,
    /// matrix-only state (hardware inputs, mix icons and outputs, order and
    /// every existing cell) is kept.
    pub fn fold_legacy(&mut self, legacy: &RoutingModel) {
        let position = |items: &[String], id: &str| items.iter().position(|x| x == id);
        let saved_inputs: Vec<String> = self.inputs.iter().map(|i| i.id.clone()).collect();
        let saved_mixes: Vec<String> = self.mixes.iter().map(|m| m.id.clone()).collect();
        let extras = self
            .inputs
            .iter()
            .filter(|input| !legacy.inputs.iter().any(|l| l.id == input.id))
            .cloned()
            .collect::<Vec<_>>();
        let mut inputs = legacy.inputs.clone();
        inputs.extend(extras);
        let mut mixes: Vec<MixDef> = legacy
            .mixes
            .iter()
            .cloned()
            .map(|mut mix| {
                if let Some(saved) = self.mixes.iter().find(|saved| saved.id == mix.id) {
                    mix.icon = saved.icon.clone();
                    mix.icon_color = saved.icon_color.clone().or(mix.icon_color);
                    mix.output_bindings = saved.output_bindings.clone();
                }
                mix
            })
            .collect();
        let legacy_inputs: Vec<String> = legacy.inputs.iter().map(|i| i.id.clone()).collect();
        let legacy_mixes: Vec<String> = legacy.mixes.iter().map(|m| m.id.clone()).collect();
        inputs.sort_by_key(|i| {
            position(&saved_inputs, &i.id)
                .or_else(|| position(&legacy_inputs, &i.id))
                .unwrap_or(usize::MAX)
        });
        mixes.sort_by_key(|m| {
            position(&saved_mixes, &m.id)
                .or_else(|| position(&legacy_mixes, &m.id))
                .unwrap_or(usize::MAX)
        });
        self.inputs = inputs;
        self.mixes = mixes;
        for (input, cells) in &legacy.routes {
            let target = self.routes.entry(input.clone()).or_default();
            for (mix, cell) in cells {
                target.entry(mix.clone()).or_insert_with(|| cell.clone());
            }
        }
        self.renumber();
    }

    /// Keep each item's `order` equal to its list position.
    pub fn renumber(&mut self) {
        for (index, input) in self.inputs.iter_mut().enumerate() {
            input.order = index as u32;
        }
        for (index, mix) in self.mixes.iter_mut().enumerate() {
            mix.order = index as u32;
        }
    }

    pub fn input(&self, id: &str) -> Option<&InputDef> {
        self.inputs.iter().find(|i| i.id == id)
    }

    pub fn input_mut(&mut self, id: &str) -> Result<&mut InputDef, SinkError> {
        self.inputs
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| SinkError::UnknownSink(id.to_string()))
    }

    /// A software channel (a virtual sink apps play into) by sink name.
    pub fn channel_mut(&mut self, name: &str) -> Result<&mut InputDef, SinkError> {
        self.input_mut(name)
            .ok()
            .filter(|i| i.kind == InputKind::Software)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))
    }

    pub fn is_channel(&self, name: &str) -> bool {
        self.input(name)
            .is_some_and(|i| i.kind == InputKind::Software)
    }

    pub fn channels(&self) -> impl Iterator<Item = &InputDef> {
        self.inputs.iter().filter(|i| i.kind == InputKind::Software)
    }

    pub fn mix(&self, id: &str) -> Option<&MixDef> {
        self.mixes.iter().find(|m| m.id == id)
    }

    pub fn mix_mut(&mut self, id: &str) -> Result<&mut MixDef, SinkError> {
        self.mixes
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| SinkError::UnknownSink(id.to_string()))
    }

    /// Add a software channel, generating a unique sink name outside the mix
    /// namespace and the reserved names. It starts in no mix.
    pub fn add_channel(
        &mut self,
        label: &str,
        icon: Option<String>,
        icon_color: Option<String>,
    ) -> Result<InputDef, SinkError> {
        use crate::persistence::channels::{MAX_CHANNELS, RESERVED_SINK_NAMES};
        let label = valid_label(label, "channel")?;
        if self.channels().count() >= MAX_CHANNELS {
            return Err(SinkError::Config(format!(
                "at most {MAX_CHANNELS} channels are supported"
            )));
        }
        let mut base = format!("sink_{}", slugify(&label, "channel"));
        if crate::persistence::buses::is_bus_name(&base) {
            // "Bus Foo" would slug into the mix namespace; step out of it.
            base = base.replacen("sink_bus_", "sink_ch_bus_", 1);
        }
        let mut name = base.clone();
        let mut counter = 2;
        while self.input(&name).is_some() || RESERVED_SINK_NAMES.contains(&name.as_str()) {
            name = format!("{base}_{counter}");
            counter += 1;
        }
        let input = InputDef {
            id: name.clone(),
            label,
            icon,
            icon_color,
            kind: InputKind::Software,
            source_name: name,
            volume_percent: default_level(),
            muted: false,
            fx: FxChain::default(),
            order: self.inputs.len() as u32,
        };
        self.inputs.push(input.clone());
        Ok(input)
    }

    /// Remove any input and its cells. The last software channel stays: apps
    /// need somewhere to play.
    pub fn remove_input(&mut self, id: &str) -> Result<InputDef, SinkError> {
        let input = self
            .input(id)
            .cloned()
            .ok_or_else(|| SinkError::UnknownSink(id.to_string()))?;
        if input.kind == InputKind::Software && self.channels().count() <= 1 {
            return Err(SinkError::Config("at least one channel is required".into()));
        }
        self.inputs.retain(|i| i.id != id);
        self.routes.remove(id);
        self.renumber();
        Ok(input)
    }

    pub fn rename_input(&mut self, id: &str, label: &str) -> Result<(), SinkError> {
        let label = valid_label(label, "channel")?;
        self.input_mut(id)?.label = label;
        Ok(())
    }

    /// Add a mix with a unique node name. It starts empty: nothing is routed
    /// into a new mix until its cells are checked.
    pub fn add_mix(&mut self, label: &str) -> Result<MixDef, SinkError> {
        use crate::persistence::buses::{BUS_PREFIX, MAX_BUSES};
        let label = valid_label(label, "mix")?;
        if self.mixes.len() >= MAX_BUSES {
            return Err(SinkError::Config(format!(
                "at most {MAX_BUSES} mixes are supported"
            )));
        }
        let base = format!("{BUS_PREFIX}{}", slugify(&label, "mix"));
        let mut id = base.clone();
        let mut counter = 2;
        while self.mix(&id).is_some() {
            id = format!("{base}_{counter}");
            counter += 1;
        }
        let mix = MixDef {
            id,
            label,
            icon: Some("broadcast".into()),
            icon_color: Some("purple".into()),
            volume_percent: default_level(),
            muted: false,
            output_bindings: Vec::new(),
            order: self.mixes.len() as u32,
        };
        self.mixes.push(mix.clone());
        Ok(mix)
    }

    pub fn remove_mix(&mut self, id: &str) -> Result<MixDef, SinkError> {
        let mix = self
            .mix(id)
            .cloned()
            .ok_or_else(|| SinkError::UnknownSink(id.to_string()))?;
        self.mixes.retain(|m| m.id != id);
        for cells in self.routes.values_mut() {
            cells.remove(id);
        }
        self.renumber();
        Ok(mix)
    }

    pub fn rename_mix(&mut self, id: &str, label: &str) -> Result<(), SinkError> {
        let label = valid_label(label, "mix")?;
        self.mix_mut(id)?.label = label;
        Ok(())
    }

    /// The inputs a mix carries: every input whose cell for it is on.
    pub fn members(&self, mix: &str) -> Vec<String> {
        self.inputs
            .iter()
            .filter(|i| self.cell(&i.id, mix).enabled)
            .map(|i| i.id.clone())
            .collect()
    }

    /// The level an input is sent into a mix at: its cell's send, or nothing
    /// when the cell is off or muted.
    pub fn member_gain(&self, input: &str, mix: &str) -> u8 {
        let cell = self.cell(input, mix);
        if cell.enabled && !cell.muted {
            cell.send_percent
        } else {
            0
        }
    }

    /// Solo `id`, or un-solo it when it already is. Soloing mutes every other
    /// input (and unmutes `id`); un-soloing puts every input's earlier mute
    /// back. Soloing a different input moves the solo, restoring first.
    /// Returns the inputs whose mute changed.
    pub fn toggle_solo(&mut self, id: &str) -> Result<Vec<String>, SinkError> {
        if self.input(id).is_none() {
            return Err(SinkError::UnknownSink(id.to_string()));
        }
        let mut changed = Vec::new();
        let mut set_muted = |inputs: &mut Vec<InputDef>, target: &str, muted: bool| {
            if let Some(input) = inputs.iter_mut().find(|i| i.id == target) {
                if input.muted != muted {
                    input.muted = muted;
                    if !changed.iter().any(|c: &String| c == target) {
                        changed.push(target.to_string());
                    }
                }
            }
        };
        let previous = self.solo.take();
        if let Some(solo) = &previous {
            // Inputs removed during the solo are simply skipped; ones added
            // during it keep whatever they are.
            for (input, muted) in &solo.restore {
                set_muted(&mut self.inputs, input, *muted);
            }
        }
        if previous.is_some_and(|solo| solo.input == id) {
            return Ok(changed);
        }
        let restore = self
            .inputs
            .iter()
            .map(|i| (i.id.clone(), i.muted))
            .collect::<BTreeMap<_, _>>();
        let others: Vec<String> = self
            .inputs
            .iter()
            .filter(|i| i.id != id)
            .map(|i| i.id.clone())
            .collect();
        for other in others {
            set_muted(&mut self.inputs, &other, true);
        }
        set_muted(&mut self.inputs, id, false);
        self.solo = Some(Solo {
            input: id.to_string(),
            restore,
        });
        Ok(changed)
    }

    /// The software channels as the strip list the UI and app routing use.
    pub fn channel_list(&self) -> Vec<crate::audio::types::VirtualSink> {
        self.channels()
            .map(|c| crate::audio::types::VirtualSink {
                name: c.id.clone(),
                label: c.label.clone(),
                icon: c.icon.clone(),
                icon_color: c.icon_color.clone(),
                volume_percent: c.volume_percent,
                muted: c.muted,
            })
            .collect()
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

    fn mix(id: &str, outputs: Vec<OutputBinding>) -> MixDef {
        MixDef {
            id: id.into(),
            label: id.into(),
            icon: None,
            icon_color: None,
            volume_percent: 100,
            muted: false,
            output_bindings: outputs,
            order: 0,
        }
    }

    #[test]
    fn a_silent_setup_gets_the_system_default_on_its_first_mix() {
        let mut model = RoutingModel {
            mixes: vec![mix("sink_bus_a", vec![]), mix("sink_bus_b", vec![])],
            ..RoutingModel::default()
        };
        assert!(model.ensure_an_output());
        assert_eq!(
            model.mixes[0].output_bindings[0].device,
            SYSTEM_DEFAULT_OUTPUT
        );
        assert!(model.mixes[1].output_bindings.is_empty());
        // Idempotent: once something plays, nothing more is added.
        assert!(!model.ensure_an_output());
    }

    #[test]
    fn a_setup_with_an_output_is_left_alone() {
        let bound = vec![OutputBinding {
            device: "alsa_output.hdmi".into(),
            enabled: true,
        }];
        let mut model = RoutingModel {
            mixes: vec![mix("sink_bus_a", vec![]), mix("sink_bus_b", bound.clone())],
            ..RoutingModel::default()
        };
        assert!(!model.ensure_an_output());
        assert!(model.mixes[0].output_bindings.is_empty());
        assert_eq!(model.mixes[1].output_bindings, bound);
    }

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

    fn legacy_with_mix() -> (Channels, Buses) {
        let buses: Buses = serde_json::from_str(
            r#"{"buses":[{"name":"sink_bus_stream","label":"Stream",
                "channels":["sink_game","sink_chat"],"exclude":false}]}"#,
        )
        .unwrap();
        (Channels::default(), buses)
    }

    #[test]
    fn cells_are_independent_from_each_other() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        model
            .set_cell(
                "sink_game",
                "sink_bus_stream",
                RouteCell {
                    enabled: true,
                    send_percent: 70,
                    muted: false,
                },
            )
            .unwrap();
        assert_eq!(model.cell("sink_game", "sink_bus_stream").send_percent, 70);
        assert!(model.cell("sink_chat", "sink_bus_stream").enabled);
        assert!(!model.cell("sink_music", "sink_bus_stream").enabled);
    }

    #[test]
    fn members_and_gains_come_from_the_cells() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        let cell = |enabled, send_percent, muted| RouteCell {
            enabled,
            send_percent,
            muted,
        };
        model
            .set_cell("sink_chat", "sink_bus_stream", cell(true, 40, false))
            .unwrap();
        model
            .set_cell("sink_music", "sink_bus_stream", cell(true, 100, true))
            .unwrap();
        assert_eq!(
            model.members("sink_bus_stream"),
            vec!["sink_game", "sink_chat", "sink_music"]
        );
        assert_eq!(model.member_gain("sink_game", "sink_bus_stream"), 100);
        assert_eq!(model.member_gain("sink_chat", "sink_bus_stream"), 40);
        assert_eq!(
            model.member_gain("sink_music", "sink_bus_stream"),
            0,
            "muted"
        );
        assert_eq!(
            model.member_gain("sink_system", "sink_bus_stream"),
            0,
            "off"
        );
    }

    // Bug shape: an auto-include mix stored its *excluded* channels, and a
    // checked cell was written there as if it were a member, so the matrix
    // showed a channel routed that the audio never carried. On migration the
    // cells (what the user saw and set) win; labels and levels come from the
    // legacy files that owned them.
    #[test]
    fn folding_legacy_keeps_the_cells_and_takes_levels() {
        let channels = Channels::default();
        let buses: Buses = serde_json::from_str(
            r#"{"buses":[{"name":"sink_bus_chat","label":"Headphones","volume_percent":76,
                "channels":["sink_game","sink_chat","sink_music"],"exclude":true}]}"#,
        )
        .unwrap();
        let legacy = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        assert!(!legacy.cell("sink_game", "sink_bus_chat").enabled);

        let mut saved = legacy.clone();
        saved.version = 1;
        saved.mixes[0].volume_percent = 100; // stale projection
        saved.mixes[0].output_bindings = vec![OutputBinding {
            device: "alsa_output.usb".into(),
            enabled: true,
        }];
        for channel in ["sink_game", "sink_chat", "sink_music", "sink_system"] {
            saved
                .set_cell(
                    channel,
                    "sink_bus_chat",
                    RouteCell {
                        enabled: true,
                        send_percent: 100,
                        muted: false,
                    },
                )
                .unwrap();
        }
        saved.fold_legacy(&legacy);

        assert_eq!(saved.members("sink_bus_chat").len(), 4, "cells win");
        assert_eq!(saved.mixes[0].volume_percent, 76, "legacy level wins");
        assert_eq!(saved.mixes[0].output_bindings.len(), 1, "matrix-only kept");
    }

    #[test]
    fn new_channels_get_unique_safe_names_and_start_unrouted() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        let added = model
            .add_channel("Voice Chat!", Some("mic".into()), None)
            .unwrap();
        assert_eq!(added.id, "sink_voice_chat");
        assert_eq!(added.icon.as_deref(), Some("mic"));
        assert!(model
            .members("sink_bus_stream")
            .iter()
            .all(|m| m != &added.id));
        let again = model.add_channel("Voice Chat", None, None).unwrap();
        assert_eq!(again.id, "sink_voice_chat_2");
        // Reserved collision: label "Mic" must not produce sink_mic.
        assert_eq!(
            model.add_channel("Mic", None, None).unwrap().id,
            "sink_mic_2"
        );
        // All-special-char labels fall back; whitespace-only is rejected.
        assert_eq!(
            model.add_channel("!!!", None, None).unwrap().id,
            "sink_channel"
        );
        assert!(model.add_channel("   ", None, None).is_err());
        // "Bus Foo" must not land in the mix namespace.
        let bus_like = model.add_channel("Bus Foo", None, None).unwrap();
        assert_eq!(bus_like.id, "sink_ch_bus_foo");
        assert!(!crate::persistence::buses::is_bus_name(&bus_like.id));
    }

    #[test]
    fn the_last_channel_stays_and_removal_drops_its_cells() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        model.remove_input("sink_game").unwrap();
        assert!(!model.routes.contains_key("sink_game"));
        assert!(!model
            .members("sink_bus_stream")
            .contains(&"sink_game".to_string()));
        model.remove_input("sink_chat").unwrap();
        model.remove_input("sink_music").unwrap();
        assert!(
            model.remove_input("sink_system").is_err(),
            "last channel stays"
        );
    }

    #[test]
    fn solo_mutes_the_others_and_unsolo_restores_them() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        model.input_mut("sink_music").unwrap().muted = true; // muted before
        model.input_mut("sink_chat").unwrap().muted = true; // the one soloed

        model.toggle_solo("sink_chat").unwrap();
        let muted = |m: &RoutingModel, id: &str| m.input(id).unwrap().muted;
        assert!(!muted(&model, "sink_chat"), "the soloed input is heard");
        assert!(muted(&model, "sink_game") && muted(&model, "sink_system"));
        assert_eq!(model.solo.as_ref().unwrap().input, "sink_chat");

        model.toggle_solo("sink_chat").unwrap();
        assert!(model.solo.is_none());
        assert!(!muted(&model, "sink_game") && !muted(&model, "sink_system"));
        assert!(
            muted(&model, "sink_music") && muted(&model, "sink_chat"),
            "earlier mutes back"
        );
    }

    #[test]
    fn soloing_another_input_moves_the_solo() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        model.toggle_solo("sink_game").unwrap();
        model.toggle_solo("sink_music").unwrap();
        let muted = |id: &str| model.input(id).unwrap().muted;
        assert!(!muted("sink_music") && muted("sink_game") && muted("sink_chat"));
        // Un-soloing returns to the state before the *first* solo: all heard.
        model.toggle_solo("sink_music").unwrap();
        assert!(model.inputs.iter().all(|i| !i.muted));
        assert!(model.toggle_solo("sink_nope").is_err());
    }

    #[test]
    fn new_mixes_start_empty_and_removal_drops_their_cells() {
        let (channels, buses) = legacy_with_mix();
        let mut model = RoutingModel::from_legacy(&channels, &buses, &ChannelOutputs::default());
        let mix = model.add_mix("Discord").unwrap();
        assert_eq!(mix.id, "sink_bus_discord");
        assert!(model.members(&mix.id).is_empty());
        model.remove_mix("sink_bus_stream").unwrap();
        assert!(model
            .routes
            .values()
            .all(|cells| !cells.contains_key("sink_bus_stream")));
        assert!(model.remove_mix("sink_bus_nope").is_err());
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
