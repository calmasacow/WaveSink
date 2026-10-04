use tauri::State;

use crate::state::AppState;

/// Create a new channel from a label and icon (sink name is generated). It
/// starts at 100%, unmuted and in no mix: tick its cells to route it.
#[tauri::command]
pub fn add_channel(
    state: State<'_, AppState>,
    label: String,
    icon: Option<String>,
    icon_color: Option<String>,
) -> Result<(), String> {
    let (channel, prefs) = {
        let mut mixer = state.lock_mixer()?;
        let channel = mixer
            .routing
            .add_channel(&label, icon, icon_color)
            .map_err(|e| e.to_string())?;
        (channel, mixer.prefs.clone())
    };

    if let Err(e) = (|| {
        state
            .backend
            .create_virtual_sink(&channel.id, &prefs.decorate(&channel.label))?;
        state.backend.set_sink_volume(&channel.id, 100)?;
        state.backend.set_sink_mute(&channel.id, false)
    })() {
        // Roll back so the model matches reality: destroy the sink if it got
        // created (idempotent if it didn't), then drop the channel.
        let _ = state.backend.destroy_virtual_sink(&channel.id);
        let mut mixer = state.lock_mixer()?;
        let _ = mixer.routing.remove_input(&channel.id);
        return Err(e.to_string());
    }

    let mixer = state.lock_mixer()?;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Apply a change to one channel's presentation and persist it.
fn edit_channel(
    state: &AppState,
    sink_name: &str,
    edit: impl FnOnce(&mut crate::routing_model::InputDef),
) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    edit(
        mixer
            .routing
            .channel_mut(sink_name)
            .map_err(|e| e.to_string())?,
    );
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Change a channel's strip icon.
#[tauri::command]
pub fn set_channel_icon(
    state: State<'_, AppState>,
    sink_name: String,
    icon: String,
) -> Result<(), String> {
    let icon = (!icon.is_empty()).then_some(icon);
    edit_channel(&state, &sink_name, |c| c.icon = icon)
}

#[tauri::command]
pub fn set_channel_icon_color(
    state: State<'_, AppState>,
    sink_name: String,
    icon_color: String,
) -> Result<(), String> {
    let color = (!icon_color.is_empty()).then_some(icon_color);
    edit_channel(&state, &sink_name, |c| c.icon_color = color)
}

/// Rename a channel's display label (the sink name stays stable, so
/// assignments and profiles keep working).
#[tauri::command]
pub fn rename_channel(
    state: State<'_, AppState>,
    sink_name: String,
    label: String,
) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    if !mixer.routing.is_channel(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    mixer
        .routing
        .rename_input(&sink_name, &label)
        .map_err(|e| e.to_string())?;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Delete a channel: streams on it return to the default sink, its
/// assignments, EQ and cells are dropped, and the sink is destroyed.
#[tauri::command]
pub fn remove_channel(state: State<'_, AppState>, sink_name: String) -> Result<(), String> {
    // Validate first (also enforces "keep one").
    {
        let mixer = state.lock_mixer()?;
        if !mixer.routing.is_channel(&sink_name) {
            return Err(format!("unknown channel: {sink_name}"));
        }
        if mixer.routing.channels().count() <= 1 {
            return Err("at least one channel is required".into());
        }
    }

    // Hand the channel's streams back to the default sink before the rug
    // is pulled out from under them.
    if let Ok(streams) = state.backend.list_app_streams() {
        for stream in streams {
            if stream.assigned_sink.as_deref() == Some(sink_name.as_str()) {
                if let Err(e) = state.backend.move_stream_to_sink(stream.index, "") {
                    eprintln!("wavesink: evacuating {} failed: {e}", stream.app_name);
                }
            }
        }
    }

    state
        .backend
        .destroy_virtual_sink(&sink_name)
        .map_err(|e| e.to_string())?;

    let (model, assignments, eq) = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .routing
            .remove_input(&sink_name)
            .map_err(|e| e.to_string())?;
        mixer
            .assignments
            .assignments
            .retain(|a| a.sink_name != sink_name);
        // The backend's DestroySink already tore down the live insert; this
        // drops the persisted config with the channel.
        mixer.eq.remove(&sink_name);
        // Re-evaluate auto-routing with the channel gone.
        mixer.auto_routed.clear();
        crate::commands::profiles::autosave_active(&mixer);
        (
            mixer.routing.clone(),
            mixer.assignments.clone(),
            mixer.eq.clone(),
        )
    };

    for mix in &model.mixes {
        let _ = crate::commands::graph::apply_mix_routes(&state, &model, &mix.id);
    }
    model.save().map_err(|e| e.to_string())?;
    assignments.save().map_err(|e| e.to_string())?;
    eq.save().map_err(|e| e.to_string())
}
