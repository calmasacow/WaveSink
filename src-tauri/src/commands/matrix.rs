use crate::commands::routing::MAX_VOLUME;
use tauri::State;

use crate::routing_model::{FxChain, InputDef, InputKind, OutputBinding, RouteCell, RoutingModel};
use crate::state::AppState;

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

/// The routing model: inputs, mixes and every cell. The one source of truth.
#[tauri::command]
pub fn get_routing_model(state: State<'_, AppState>) -> Result<RoutingModel, String> {
    Ok(state.lock_mixer()?.routing.clone())
}

fn reorder<T: HasId>(items: &mut [T], order: &[String]) -> Result<(), String> {
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
    mixer.routing.renumber();
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

#[tauri::command]
pub fn reorder_matrix_mixes(state: State<'_, AppState>, order: Vec<String>) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    reorder(&mut mixer.routing.mixes, &order)?;
    mixer.routing.renumber();
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
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
    // It starts in no mix (an absent cell is off): tick its cells to route it.
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
    let model = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .routing
            .set_cell(
                &input_id,
                &mix_id,
                RouteCell {
                    enabled,
                    send_percent: send_percent.min(MAX_VOLUME),
                    muted,
                },
            )
            .map_err(|e| e.to_string())?;
        mixer.routing.save().map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.routing.clone()
    };
    crate::commands::graph::apply_mix_routes(&state, &model, &mix_id).map_err(|e| e.to_string())
}

/// Solo an input (every other input muted) or, when it already is, un-solo it
/// and put the earlier mutes back. Bound to right-clicking an input's icon.
#[tauri::command]
pub fn toggle_solo(state: State<'_, AppState>, input_id: String) -> Result<(), String> {
    let (model, changed) = {
        let mut mixer = state.lock_mixer()?;
        let changed = mixer
            .routing
            .toggle_solo(&input_id)
            .map_err(|e| e.to_string())?;
        mixer.routing.save().map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        (mixer.routing.clone(), changed)
    };
    for id in changed {
        let Some(input) = model.input(&id) else {
            continue;
        };
        let applied = match input.kind {
            InputKind::Software => state.backend.set_sink_mute(&input.id, input.muted),
            InputKind::Hardware => state.backend.set_hardware_input(
                &input.id,
                &input.source_name,
                input.volume_percent,
                input.muted,
            ),
        };
        applied.map_err(|e| e.to_string())?;
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
        input.volume_percent = volume_percent.min(MAX_VOLUME);
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
    mixer
        .routing
        .remove_input(&input_id)
        .map_err(|e| e.to_string())?;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    drop(mixer);
    state
        .backend
        .remove_hardware_input(&input_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_mix_outputs(
    state: State<'_, AppState>,
    mix_id: String,
    outputs: Vec<OutputBinding>,
) -> Result<(), String> {
    if state.lock_mixer()?.routing.mix(&mix_id).is_none() {
        return Err(format!("unknown mix {mix_id}"));
    }
    state
        .backend
        .set_mix_outputs(&mix_id, &outputs)
        .map_err(|e| e.to_string())?;
    let mut mixer = state.lock_mixer()?;
    mixer
        .routing
        .mix_mut(&mix_id)
        .map_err(|e| e.to_string())?
        .output_bindings = outputs;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

#[tauri::command]
pub fn set_input_fx(
    state: State<'_, AppState>,
    input_id: String,
    fx: FxChain,
) -> Result<(), String> {
    // Clamped before it is stored, so the file never holds what the DSP
    // would refuse.
    let fx = fx.clamped();
    let hardware = {
        let mut mixer = state.lock_mixer()?;
        let input = mixer
            .routing
            .inputs
            .iter_mut()
            .find(|input| input.id == input_id)
            .ok_or_else(|| format!("unknown input {input_id}"))?;
        input.fx = fx.clone();
        let hardware = input.kind == InputKind::Hardware;
        mixer.routing.save().map_err(|e| e.to_string())?;
        hardware
    };
    // Audio FX runs on hardware inputs; software channels have their EQ.
    if hardware {
        state
            .backend
            .set_input_fx(&input_id, &fx)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
