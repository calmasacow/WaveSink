use tauri::State;

use crate::persistence::profiles::{self, Profile, ProfileInfo};
use crate::routing_model::InputKind;
use crate::state::AppState;

/// Persist the current mixer state into the active profile, if any.
/// Profiles are live-bound, so switching away and back never loses changes.
pub fn autosave_active(mixer: &crate::mixer::state::MixerState) {
    let Some(name) = &mixer.active_profile else {
        return;
    };
    // The trigger comes from the cache rather than a disk re-read each mutation.
    let profile = Profile::snapshot(name, mixer, mixer.active_trigger.clone());
    if let Err(e) = profiles::save(&profile) {
        eprintln!("wavesink: autosave of profile {name} failed: {e}");
    }
}

fn set_active(state: &AppState, name: Option<String>) -> Result<(), String> {
    // Refresh the cached trigger from the profile we're binding to (a rare
    // profile switch, not the per-mutation autosave path).
    let trigger = name
        .as_deref()
        .and_then(|n| profiles::load(n).ok())
        .and_then(|p| p.trigger_device);
    let mut mixer = state.lock_mixer()?;
    mixer.active_profile = name.clone();
    mixer.active_trigger = trigger;
    crate::persistence::active::save(name.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_profiles() -> Result<Vec<ProfileInfo>, String> {
    profiles::list().map_err(|e| e.to_string())
}

/// The profile changes are currently autosaving into (restored at launch).
#[tauri::command]
pub fn get_active_profile(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.active_profile.clone())
}

/// Bind (or clear, with empty string) an output device that auto-loads
/// this profile when it appears.
#[tauri::command]
pub fn set_profile_trigger(
    state: State<'_, AppState>,
    name: String,
    device: String,
) -> Result<(), String> {
    let trigger = if device.is_empty() {
        None
    } else {
        Some(device)
    };
    profiles::set_trigger(&name, trigger.clone()).map_err(|e| e.to_string())?;
    // Keep the cache in step so a later autosave doesn't overwrite the trigger
    // we just set on the active profile with a stale value.
    let mut mixer = state.lock_mixer()?;
    if mixer.active_profile.as_deref() == Some(name.as_str()) {
        mixer.active_trigger = trigger;
    }
    Ok(())
}

/// Apply a saved profile: tear down what it doesn't have, bring up its whole
/// graph, then clear the auto-route ledger so routing re-enforces.
#[tauri::command]
pub fn load_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    load_profile_on(&state, name)?;
    crate::refresh_tray(&app);
    Ok(())
}

pub fn load_profile_on(state: &AppState, name: String) -> Result<(), String> {
    // A tray click, a hotkey and the UI can all land here at once; the
    // reconcile below reads and writes the mixer in several steps.
    let _switching = state
        .profile_switch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let profile = profiles::load(&name).map_err(|e| e.to_string())?;
    let target = profile.routing.clone();
    if target.channels().next().is_none() {
        return Err(format!("profile {name} has no channels"));
    }
    let _rebuild = state.lock_bus_rebuild();
    let (current, prefs) = {
        let mixer = state.lock_mixer()?;
        (mixer.routing.clone(), mixer.prefs.clone())
    };

    // ---- tear down what the profile doesn't have ----
    for old in current.channels() {
        if target.is_channel(&old.id) {
            continue;
        }
        // Evacuate this channel's streams before destroying it.
        if let Ok(streams) = state.backend.list_app_streams() {
            for stream in streams {
                if stream.assigned_sink.as_deref() == Some(old.id.as_str()) {
                    let _ = state.backend.move_stream_to_sink(stream.index, "");
                }
            }
        }
        if let Err(e) = state.backend.destroy_virtual_sink(&old.id) {
            eprintln!("wavesink: removing {} for profile failed: {e}", old.id);
        }
    }
    for old in current
        .inputs
        .iter()
        .filter(|i| i.kind == InputKind::Hardware && target.input(&i.id).is_none())
    {
        if let Err(e) = state.backend.remove_hardware_input(&old.id) {
            eprintln!("wavesink: removing {} for profile failed: {e}", old.id);
        }
    }
    for old in &current.mixes {
        if target.mix(&old.id).is_none() {
            if let Err(e) = state.backend.destroy_bus(&old.id) {
                eprintln!("wavesink: removing mix {} for profile failed: {e}", old.id);
            }
        }
    }

    // ---- bring up everything it does ----
    crate::commands::graph::bring_up(state, &target, &profile.eq, &prefs)?;

    let assignments = {
        let mut mixer = state.lock_mixer()?;
        mixer.routing = target.clone();
        mixer.assignments = profile.assignments.clone();
        mixer.eq = profile.eq.clone();
        mixer.auto_routed.clear();
        mixer.assignments.clone()
    };
    target.save().map_err(|e| e.to_string())?;
    assignments.save().map_err(|e| e.to_string())?;
    profile.eq.save().map_err(|e| e.to_string())?;
    // The loaded profile becomes the live-bound (autosaving) one.
    set_active(state, Some(name))?;
    Ok(())
}

/// Create a profile with a clean slate: the classic four channels at
/// 100%/unmuted in one mix. Saved but not applied - load it to start fresh.
#[tauri::command]
pub fn create_blank_profile(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let name = profiles::sanitize_name(&name).map_err(|e| e.to_string())?;
    if profiles::load(&name).is_ok() {
        return Err(format!("profile \"{name}\" already exists"));
    }
    // The starter layout: the classic channels, all in one mix that plays
    // to the system default output.
    let mut routing = crate::routing_model::RoutingModel::from_legacy(
        &crate::persistence::channels::Channels::default(),
        &crate::persistence::buses::Buses::default(),
        &Default::default(),
    );
    routing.ensure_an_output();
    let profile = Profile {
        name,
        routing,
        assignments: Default::default(),
        eq: Default::default(),
        trigger_device: None,
        channels: Vec::new(),
        buses: Default::default(),
        outputs: Default::default(),
    };
    profiles::save(&profile).map_err(|e| e.to_string())?;
    crate::refresh_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    profiles::delete(&name).map_err(|e| e.to_string())?;
    let is_active = {
        let mixer = state.lock_mixer()?;
        mixer.active_profile.as_deref() == Some(name.as_str())
    };
    if is_active {
        set_active(&state, None)?;
    }
    crate::refresh_tray(&app);
    Ok(())
}
