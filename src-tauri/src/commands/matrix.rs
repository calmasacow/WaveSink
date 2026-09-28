use tauri::State;

use crate::routing_model::{FxChain, InputDef, InputKind, OutputBinding, RouteCell, RoutingModel};
use crate::state::AppState;

/// Legacy buses own membership and levels while the graph migration runs.
/// Matrix-only fields must survive that projection on every read.
fn project_legacy(model: &mut RoutingModel, legacy: &RoutingModel) {
    let saved_inputs = model.inputs.clone();
    let saved_mixes = model.mixes.clone();
    let extras = model
        .inputs
        .iter()
        .filter(|input| {
            !legacy
                .inputs
                .iter()
                .any(|legacy_input| legacy_input.id == input.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    model.inputs = legacy.inputs.clone();
    model.inputs.extend(extras);
    model.mixes = legacy
        .mixes
        .iter()
        .cloned()
        .map(|mut mix| {
            if let Some(saved) = model.mixes.iter().find(|saved| saved.id == mix.id) {
                mix.icon = saved.icon.clone();
                // Old routing files have no mix color. Preserve the normalized
                // bus default instead of replacing it with a missing value.
                mix.icon_color = saved.icon_color.clone().or(mix.icon_color);
                mix.output_bindings = saved.output_bindings.clone();
            }
            mix
        })
        .collect();
    sort_by_saved_order(&mut model.inputs, &legacy.inputs, &saved_inputs);
    sort_by_saved_order(&mut model.mixes, &legacy.mixes, &saved_mixes);
}

fn sort_by_saved_order<T>(items: &mut [T], legacy: &[T], saved: &[T])
where
    T: HasId,
{
    items.sort_by_key(|item| {
        saved
            .iter()
            .position(|saved| saved.id() == item.id())
            .or_else(|| legacy.iter().position(|legacy| legacy.id() == item.id()))
            .unwrap_or(usize::MAX)
    });
}

trait HasId {
    fn id(&self) -> &str;
}
impl HasId for InputDef {
    fn id(&self) -> &str {
        &self.id
    }
}
impl HasId for crate::routing_model::MixDef {
    fn id(&self) -> &str {
        &self.id
    }
}

/// Return the matrix projection used by the routing-table UI and future
/// control-surface APIs. Legacy channel/bus edits are folded in on read so an
/// older profile or a tray action cannot leave the table stale.
#[tauri::command]
pub fn get_routing_model(state: State<'_, AppState>) -> Result<RoutingModel, String> {
    let mixer = state.lock_mixer()?;
    let legacy = RoutingModel::from_legacy(&mixer.channel_defs, &mixer.buses, &mixer.outputs);
    let mut model = mixer.routing.clone();
    project_legacy(&mut model, &legacy);
    for (input, cells) in legacy.routes {
        let target = model.routes.entry(input).or_default();
        for (mix, cell) in cells {
            target.entry(mix).or_insert(cell);
        }
    }
    Ok(model)
}

fn reorder<T: HasId>(items: &mut Vec<T>, order: &[String]) -> Result<(), String> {
    if order.len() != items.len()
        || order
            .iter()
            .any(|id| !items.iter().any(|item| item.id() == id))
        || order.iter().collect::<std::collections::HashSet<_>>().len() != order.len()
    {
        return Err("order must list every item exactly once".into());
    }
    items.sort_by_key(|item| {
        order
            .iter()
            .position(|id| id == item.id())
            .expect("validated")
    });
    Ok(())
}

#[tauri::command]
pub fn reorder_matrix_inputs(state: State<'_, AppState>, order: Vec<String>) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    reorder(&mut mixer.routing.inputs, &order)?;
    for (index, input) in mixer.routing.inputs.iter_mut().enumerate() {
        input.order = index as u32;
    }
    mixer.routing.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_matrix_mixes(state: State<'_, AppState>, order: Vec<String>) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    reorder(&mut mixer.routing.mixes, &order)?;
    for (index, mix) in mixer.routing.mixes.iter_mut().enumerate() {
        mix.order = index as u32;
    }
    mixer.routing.save().map_err(|e| e.to_string())
}

/// Add a hardware source to the matrix. It remains present while disconnected
/// so its saved sends can be restored when PipeWire reports it again.
#[tauri::command]
pub fn add_hardware_input(
    state: State<'_, AppState>,
    source_name: String,
    label: String,
    icon: Option<String>,
    icon_color: Option<String>,
) -> Result<(), String> {
    let devices = state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())?;
    if crate::audio::types::is_own_sink(&source_name) {
        return Err("WaveSink virtual sources cannot be hardware inputs".into());
    }
    if !devices.iter().any(|device| device.name == source_name) {
        return Err("hardware input is no longer available".into());
    }
    let label = label.trim();
    if label.is_empty() || label.len() > 24 {
        return Err("input label must be 1-24 characters".into());
    }
    let mut mixer = state.lock_mixer()?;
    if mixer
        .routing
        .inputs
        .iter()
        .any(|input| input.source_name == source_name)
    {
        return Err("hardware input is already in the matrix".into());
    }
    let id = format!("hardware:{source_name}");
    let order = mixer.routing.inputs.len() as u32;
    let mixes = mixer.routing.mixes.clone();
    mixer.routing.inputs.push(InputDef {
        id: id.clone(),
        label: label.to_string(),
        icon,
        icon_color,
        kind: InputKind::Hardware,
        source_name,
        volume_percent: 100,
        muted: false,
        fx: FxChain::default(),
        order,
    });
    mixer.routing.routes.insert(
        id,
        mixes
            .into_iter()
            .map(|mix| (mix.id, RouteCell::default()))
            .collect(),
    );
    state
        .backend
        .set_hardware_input(
            &mixer.routing.inputs.last().expect("inserted input").id,
            &mixer
                .routing
                .inputs
                .last()
                .expect("inserted input")
                .source_name,
            100,
            false,
        )
        .map_err(|e| e.to_string())?;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Change one independent input×mix send. This is the central matrix command:
/// it never changes the input fader and never touches another mix's cell.
#[tauri::command]
pub fn set_route_cell(
    state: State<'_, AppState>,
    input_id: String,
    mix_id: String,
    enabled: bool,
    send_percent: u8,
    muted: bool,
) -> Result<(), String> {
    let cell = RouteCell {
        enabled,
        send_percent: send_percent.min(150),
        muted,
    };
    let (def, bus_channels, hardware) = {
        let mut mixer = state.lock_mixer()?;
        let mut model = mixer.routing.clone();
        let legacy = RoutingModel::from_legacy(&mixer.channel_defs, &mixer.buses, &mixer.outputs);
        project_legacy(&mut model, &legacy);
        model
            .set_cell(&input_id, &mix_id, cell.clone())
            .map_err(|e| e.to_string())?;
        let hardware = model
            .inputs
            .iter()
            .any(|input| input.id == input_id && input.kind == InputKind::Hardware);
        model.save().map_err(|e| e.to_string())?;
        mixer.routing = model;
        let all = mixer
            .channel_defs
            .channels
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        let def = mixer.buses.get(&mix_id).cloned();
        let mut members = def
            .as_ref()
            .map(|d| d.effective_members(&all))
            .unwrap_or_default();
        if enabled && !members.contains(&input_id) && input_id != "sink_mic" {
            members.push(input_id.clone());
        }
        if !enabled && input_id != "sink_mic" {
            members.retain(|m| m != &input_id);
        }
        (def, members, hardware)
    };

    if def.is_some() {
        if input_id == "sink_mic" {
            state
                .backend
                .set_bus_mic(&mix_id, enabled)
                .map_err(|e| e.to_string())?;
        } else {
            state
                .backend
                .set_bus_members(&mix_id, &bus_channels)
                .map_err(|e| e.to_string())?;
        }
        let effective_gain = if muted || !enabled {
            0
        } else {
            cell.send_percent
        };
        state
            .backend
            .set_bus_member_gain(&mix_id, &input_id, effective_gain)
            .map_err(|e| e.to_string())?;
        let mut mixer = state.lock_mixer()?;
        if input_id != "sink_mic" && !hardware {
            mixer
                .buses
                .set_members(&mix_id, bus_channels)
                .map_err(|e| e.to_string())?;
        }
        mixer
            .buses
            .set_member_gain(&mix_id, &input_id, effective_gain)
            .map_err(|e| e.to_string())?;
        mixer.buses.save().map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
    } else {
        return Err(format!("unknown mix {mix_id}"));
    }
    Ok(())
}

#[tauri::command]
pub fn set_input_level(
    state: State<'_, AppState>,
    input_id: String,
    volume_percent: u8,
    muted: bool,
) -> Result<(), String> {
    let (source_name, kind) = {
        let mut mixer = state.lock_mixer()?;
        let input = mixer
            .routing
            .inputs
            .iter_mut()
            .find(|input| input.id == input_id)
            .ok_or_else(|| format!("unknown input {input_id}"))?;
        input.volume_percent = volume_percent.min(150);
        input.muted = muted;
        let source_name = input.source_name.clone();
        let kind = input.kind.clone();
        mixer.routing.save().map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        (source_name, kind)
    };
    if kind == InputKind::Hardware {
        state
            .backend
            .set_hardware_input(&input_id, &source_name, volume_percent, muted)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Update presentation and source binding for a matrix hardware input. Routes,
/// processing, and level state stay attached to the stable input id.
#[tauri::command]
pub fn update_hardware_input(
    state: State<'_, AppState>,
    input_id: String,
    label: String,
    icon: Option<String>,
    icon_color: Option<String>,
    source_name: String,
) -> Result<(), String> {
    let devices = state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())?;
    if crate::audio::types::is_own_sink(&source_name) {
        return Err("WaveSink virtual sources cannot be hardware inputs".into());
    }
    if !devices.iter().any(|device| device.name == source_name) {
        return Err("hardware input is no longer available".into());
    }
    let label = label.trim();
    if label.is_empty() || label.len() > 24 {
        return Err("input label must be 1-24 characters".into());
    }
    let (volume, muted) = {
        let mut mixer = state.lock_mixer()?;
        if mixer.routing.inputs.iter().any(|input| {
            input.id != input_id
                && input.kind == InputKind::Hardware
                && input.source_name == source_name
        }) {
            return Err("hardware input is already in the matrix".into());
        }
        let input = mixer
            .routing
            .inputs
            .iter_mut()
            .find(|input| input.id == input_id)
            .filter(|input| input.kind == InputKind::Hardware)
            .ok_or_else(|| format!("unknown hardware input {input_id}"))?;
        input.label = label.to_string();
        input.icon = icon;
        input.icon_color = icon_color;
        input.source_name = source_name.clone();
        let level = (input.volume_percent, input.muted);
        mixer.routing.save().map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        level
    };
    state
        .backend
        .set_hardware_input(&input_id, &source_name, volume, muted)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_hardware_input(state: State<'_, AppState>, input_id: String) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    mixer
        .routing
        .inputs
        .iter()
        .find(|input| input.id == input_id)
        .filter(|input| input.kind == InputKind::Hardware)
        .cloned()
        .ok_or_else(|| format!("unknown hardware input {input_id}"))?;
    mixer.routing.inputs.retain(|item| item.id != input_id);
    mixer.routing.routes.remove(&input_id);
    mixer.buses.remove_channel(&input_id);
    if input_id == "sink_mic" {
        mixer.mic.enabled = false;
        crate::persistence::mic::save(&mixer.mic).map_err(|e| e.to_string())?;
        state
            .backend
            .set_mic_config(&mixer.mic)
            .map_err(|e| e.to_string())?;
    }
    mixer.routing.save().map_err(|e| e.to_string())?;
    mixer.buses.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    drop(mixer);
    state
        .backend
        .remove_hardware_input(&input_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_mix_monitor(state: State<'_, AppState>, mix_id: String) -> Result<(), String> {
    let previous = {
        let mut mixer = state.lock_mixer()?;
        if !mixer.buses.buses.iter().any(|mix| mix.name == mix_id) {
            return Err(format!("unknown mix {mix_id}"));
        }
        if !mixer.routing.mixes.iter().any(|mix| mix.id == mix_id) {
            mixer.routing.mixes =
                RoutingModel::from_legacy(&mixer.channel_defs, &mixer.buses, &mixer.outputs).mixes;
        }
        let previous = mixer.routing.monitor_mix.replace(mix_id.clone());
        mixer.routing.save().map_err(|e| e.to_string())?;
        previous
    };
    if let Some(previous) = previous {
        let _ = state.backend.set_monitor(&previous, false);
    }
    state
        .backend
        .set_monitor(&mix_id, true)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_mix_monitor(state: State<'_, AppState>) -> Result<(), String> {
    let previous = {
        let mut mixer = state.lock_mixer()?;
        let previous = mixer.routing.monitor_mix.take();
        mixer.routing.save().map_err(|e| e.to_string())?;
        previous
    };
    if let Some(previous) = previous {
        state
            .backend
            .set_monitor(&previous, false)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_mix_outputs(
    state: State<'_, AppState>,
    mix_id: String,
    outputs: Vec<OutputBinding>,
) -> Result<(), String> {
    {
        let mixer = state.lock_mixer()?;
        if !mixer.buses.buses.iter().any(|mix| mix.name == mix_id) {
            return Err(format!("unknown mix {mix_id}"));
        }
    }
    state
        .backend
        .set_mix_outputs(&mix_id, &outputs)
        .map_err(|e| e.to_string())?;
    let mut mixer = state.lock_mixer()?;
    if !mixer.routing.mixes.iter().any(|mix| mix.id == mix_id) {
        mixer.routing.mixes =
            RoutingModel::from_legacy(&mixer.channel_defs, &mixer.buses, &mixer.outputs).mixes;
    }
    let mix = mixer
        .routing
        .mixes
        .iter_mut()
        .find(|m| m.id == mix_id)
        .ok_or_else(|| format!("unknown mix {mix_id}"))?;
    mix.output_bindings = outputs;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

#[tauri::command]
pub fn set_hidden_devices(state: State<'_, AppState>, devices: Vec<String>) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    mixer.routing.hidden_devices = devices;
    mixer.routing.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_input_fx(
    state: State<'_, AppState>,
    input_id: String,
    fx: FxChain,
) -> Result<(), String> {
    let mic_config = {
        let mut mixer = state.lock_mixer()?;
        let input = mixer
            .routing
            .inputs
            .iter_mut()
            .find(|input| input.id == input_id)
            .ok_or_else(|| format!("unknown input {input_id}"))?;
        input.fx = fx.clone();
        let mic_config = if input_id == "sink_mic" {
            mixer.mic.gate_enabled = fx.gate_enabled;
            mixer.mic.comp_enabled = fx.compressor_enabled;
            mixer.mic.limiter_enabled = fx.limiter_enabled;
            Some(mixer.mic.clone())
        } else {
            None
        };
        mixer.routing.save().map_err(|e| e.to_string())?;
        mic_config
    };
    if let Some(config) = mic_config {
        state
            .backend
            .set_mic_config(&config)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
