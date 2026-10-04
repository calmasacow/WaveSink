use tauri::State;

use crate::audio::backend::AudioBackend;
use crate::commands::routing::MAX_VOLUME;
use crate::persistence::buses::{is_bus_name, BusDef};
use crate::state::AppState;

/// Re-apply a mix's persisted volume/mute to its node: bus nodes are born at
/// unity/unmuted, and a routing failure shouldn't abort bringing the mix up.
pub(crate) fn apply_bus_level(backend: &dyn AudioBackend, def: &BusDef) {
    if def.volume_percent != 100 {
        let _ = backend.set_sink_volume(&def.name, def.volume_percent);
    }
    if def.muted {
        let _ = backend.set_sink_mute(&def.name, true);
    }
}

/// Restore persisted send levels on a fresh/recreated bus (the
/// `apply_bus_level` rationale).
pub(crate) fn apply_bus_member_gains(backend: &dyn AudioBackend, def: &BusDef) {
    for (member, percent) in &def.member_gains {
        let _ = backend.set_bus_member_gain(&def.name, member, *percent);
    }
}

/// The user's mixes (buses) with their member channels.
#[tauri::command]
pub fn list_buses(state: State<'_, AppState>) -> Result<Vec<BusDef>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.buses.buses.clone())
}

/// Create a new mix. Recorders see it under `label`. New mixes carry
/// every channel (auto-include) until the user unchecks some.
#[tauri::command]
pub fn add_bus(state: State<'_, AppState>, label: String) -> Result<(), String> {
    let (def, defs, prefs, all) = {
        let mut mixer = state.lock_mixer()?;
        let def = mixer.buses.add(&label).map_err(|e| e.to_string())?;
        (
            def,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };
    if let Err(e) = state
        .backend
        .create_bus(&def.name, &prefs.decorate(&def.label))
    {
        let mut mixer = state.lock_mixer()?;
        let _ = mixer.buses.remove(&def.name);
        return Err(e.to_string());
    }
    if let Err(e) =
        crate::commands::buses::push_bus_members(&state, &def.name, &def.effective_members(&all))
    {
        eprintln!("wavesink: members for new mix {} failed: {e}", def.name);
    }
    defs.save().map_err(|e| e.to_string())?;
    let mixer = state.lock_mixer()?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Rename a mix by recreating its node - the node name itself stays stable,
/// so OBS configs keep working and capture re-attaches automatically.
#[tauri::command]
pub fn rename_bus(state: State<'_, AppState>, name: String, label: String) -> Result<(), String> {
    rename_bus_on(&state, name, label)
}

#[tauri::command]
pub fn set_bus_icon(state: State<'_, AppState>, name: String, icon: String) -> Result<(), String> {
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_icon(&name, icon.clone())
            .map_err(|e| e.to_string())?;
        if let Some(mix) = mixer.routing.mixes.iter_mut().find(|mix| mix.id == name) {
            mix.icon = Some(icon);
            mixer.routing.save().map_err(|e| e.to_string())?;
        }
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_bus_icon_color(
    state: State<'_, AppState>,
    name: String,
    icon_color: String,
) -> Result<(), String> {
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_icon_color(&name, icon_color.clone())
            .map_err(|e| e.to_string())?;
        if let Some(mix) = mixer.routing.mixes.iter_mut().find(|mix| mix.id == name) {
            mix.icon_color = Some(icon_color);
            mixer.routing.save().map_err(|e| e.to_string())?;
        }
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

pub fn rename_bus_on(state: &AppState, name: String, label: String) -> Result<(), String> {
    let _rebuild = state.lock_bus_rebuild();
    let (def, defs, prefs, all) = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .rename(&name, &label)
            .map_err(|e| e.to_string())?;
        let def = mixer
            .buses
            .get(&name)
            .cloned()
            .ok_or_else(|| "unknown mix".to_string())?;
        (
            def,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };

    state
        .backend
        .destroy_bus(&name)
        .map_err(|e| e.to_string())?;
    state
        .backend
        .create_bus(&def.name, &prefs.decorate(&def.label))
        .map_err(|e| e.to_string())?;
    crate::commands::buses::push_bus_members(&state, &def.name, &def.effective_members(&all))
        .map_err(|e| e.to_string())?;
    // The node is fresh; restore its saved level and send gains.
    apply_bus_level(state.backend.as_ref(), &def);
    apply_bus_member_gains(state.backend.as_ref(), &def);

    defs.save().map_err(|e| e.to_string())?;
    let mixer = state.lock_mixer()?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Delete a mix.
#[tauri::command]
pub fn remove_bus(state: State<'_, AppState>, name: String) -> Result<(), String> {
    remove_bus_on(&state, name)
}

pub fn remove_bus_on(state: &AppState, name: String) -> Result<(), String> {
    let _rebuild = state.lock_bus_rebuild();
    // Validate before the node goes away - rejecting afterwards would leave
    // the mix torn down in PipeWire but still defined here.
    state
        .lock_mixer()?
        .buses
        .removable(&name)
        .map_err(|e| e.to_string())?;
    state
        .backend
        .destroy_bus(&name)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.remove(&name).map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Hardware inputs routed into `mix`. They live only in the routing matrix:
/// a mix's saved members are software channels.
pub(crate) fn hardware_members(
    routing: &crate::routing_model::RoutingModel,
    mix: &str,
) -> Vec<String> {
    routing
        .inputs
        .iter()
        .filter(|input| input.kind == crate::routing_model::InputKind::Hardware)
        .filter(|input| {
            routing
                .routes
                .get(&input.id)
                .and_then(|cells| cells.get(mix))
                .is_some_and(|cell| cell.enabled)
        })
        .map(|input| input.id.clone())
        .collect()
}

/// Push a mix's members to the backend, adding the hardware inputs routed
/// into it. Every membership push goes through here: passing only the saved
/// software channels would silently unlink hardware inputs (a mic routed to
/// a mix stayed silent after restart until its cell was toggled).
pub(crate) fn push_bus_members(
    state: &AppState,
    mix: &str,
    channels: &[String],
) -> Result<(), crate::error::SinkError> {
    let mut members = channels.to_vec();
    if let Ok(mixer) = state.lock_mixer() {
        for id in hardware_members(&mixer.routing, mix) {
            if !members.contains(&id) {
                members.push(id);
            }
        }
    }
    state.backend.set_bus_members(mix, &members)
}

/// Replace the channel set a mix carries. For auto-include mixes the stored
/// value is the complement (unchecked set), so future channels keep flowing in.
#[tauri::command]
pub fn set_bus_members(
    state: State<'_, AppState>,
    name: String,
    channels: Vec<String>,
) -> Result<(), String> {
    // Validate against the definition set, so a rejected request never reaches
    // the backend - membership and the persisted definition could diverge.
    let stored = {
        let mixer = state.lock_mixer()?;
        let Some(def) = mixer.buses.get(&name) else {
            return Err("unknown mix".to_string());
        };
        if def.exclude {
            channel_names(&mixer)
                .into_iter()
                .filter(|c| !channels.contains(c))
                .collect()
        } else {
            channels.clone()
        }
    };
    crate::commands::buses::push_bus_members(&state, &name, &channels)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_members(&name, stored)
            .map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Switch a mix between manual selection and auto-include mode. The
/// carried set is preserved; only what happens to future channels changes.
#[tauri::command]
pub fn set_bus_exclude(
    state: State<'_, AppState>,
    name: String,
    exclude: bool,
) -> Result<(), String> {
    let defs = {
        let mut mixer = state.lock_mixer()?;
        let all = channel_names(&mixer);
        mixer
            .buses
            .set_exclude(&name, exclude, &all)
            .map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Set a mix's playback level (0-100%) - what recorders hear. Unlike
/// `set_channel_volume`, this accepts mix nodes, including the master mix.
#[tauri::command]
pub fn set_bus_volume(state: State<'_, AppState>, name: String, volume: u8) -> Result<(), String> {
    if !is_bus_name(&name) {
        return Err(format!("unknown mix: {name}"));
    }
    let volume = volume.min(MAX_VOLUME);
    state
        .backend
        .set_sink_volume(&name, volume)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_volume(&name, volume)
            .map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// Mute or unmute a mix for recorders. Persisted, and accepts the master mix.
#[tauri::command]
pub fn set_bus_mute(state: State<'_, AppState>, name: String, muted: bool) -> Result<(), String> {
    if !is_bus_name(&name) {
        return Err(format!("unknown mix: {name}"));
    }
    state
        .backend
        .set_sink_mute(&name, muted)
        .map_err(|e| e.to_string())?;
    let defs = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_muted(&name, muted)
            .map_err(|e| e.to_string())?;
        if let Some(mix) = mixer.routing.mixes.iter_mut().find(|mix| mix.id == name) {
            mix.muted = muted;
            mixer.routing.save().map_err(|e| e.to_string())?;
        }
        crate::commands::profiles::autosave_active(&mixer);
        mixer.buses.clone()
    };
    defs.save().map_err(|e| e.to_string())
}

/// The current channel sink names (the "all channels" set for mixes).
pub(crate) fn channel_names(mixer: &crate::mixer::state::MixerState) -> Vec<String> {
    mixer.channels.iter().map(|c| c.name.clone()).collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn hardware_members_lists_enabled_hardware_routes_only() {
        use crate::routing_model::{FxChain, InputDef, InputKind, RouteCell, RoutingModel};
        let input = |id: &str, kind: InputKind| InputDef {
            id: id.into(),
            label: id.into(),
            icon: None,
            icon_color: None,
            kind,
            source_name: id.into(),
            volume_percent: 100,
            muted: false,
            fx: FxChain::default(),
            order: 0,
        };
        let cell = |enabled| RouteCell {
            enabled,
            send_percent: 100,
            muted: false,
        };
        let mut model = RoutingModel::default();
        model.inputs = vec![
            input("hardware:mic", InputKind::Hardware),
            input("hardware:cam", InputKind::Hardware),
            input("sink_game", InputKind::Software),
        ];
        for (id, enabled) in [
            ("hardware:mic", true),
            ("hardware:cam", false),
            ("sink_game", true),
        ] {
            model
                .routes
                .entry(id.into())
                .or_default()
                .insert("sink_bus_stream".into(), cell(enabled));
        }
        assert_eq!(
            hardware_members(&model, "sink_bus_stream"),
            vec!["hardware:mic"]
        );
        assert!(hardware_members(&model, "sink_bus_chat").is_empty());
    }
    use super::*;
    use crate::audio::mock::{Call, MockBackend};
    use crate::persistence::testing::TempConfig;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    fn state_with_mix(backend: Arc<MockBackend>) -> (AppState, String) {
        let state = AppState::new(backend);
        let name = {
            let mut mixer = state.lock_mixer().expect("mixer");
            mixer.init_defaults();
            mixer.buses.add("Solo").expect("add mix").name
        };
        (state, name)
    }

    /// Parks the first rebuild between its destroy and its create, and signals
    /// once parked, so a second rebuild can start while the first is open.
    fn park_first_rebuild(backend: &MockBackend) -> Arc<Barrier> {
        let inside = Arc::new(Barrier::new(2));
        let signal = inside.clone();
        let parked = AtomicBool::new(false);
        backend.on_bus(move |call| {
            if matches!(call, Call::DestroyBus(_)) && !parked.swap(true, Ordering::SeqCst) {
                signal.wait();
                thread::sleep(Duration::from_millis(150));
            }
        });
        inside
    }

    fn assert_rebuilds_do_not_interleave(ops: &[Call], bus: &str) {
        let mut open = false;
        for op in ops {
            match op {
                Call::DestroyBus(n) if n == bus => {
                    assert!(
                        !open,
                        "{bus} destroyed twice before it was recreated: {ops:?}"
                    );
                    open = true;
                }
                Call::CreateBus(n) if n == bus => {
                    assert!(open, "{bus} created without a destroy first: {ops:?}");
                    open = false;
                }
                _ => {}
            }
        }
        assert!(!open, "{bus} left destroyed: {ops:?}");
    }

    // Bug shape: two rebuilds of one mix (a rename racing another rename, or a
    // profile switch) must not interleave their destroy and create.
    #[test]
    fn concurrent_renames_do_not_interleave_rebuilds() {
        let cfg = TempConfig::new("bus-rebuild-rename");
        let backend = Arc::new(MockBackend::default());
        let (state, name) = state_with_mix(backend.clone());
        let inside = park_first_rebuild(&backend);

        thread::scope(|s| {
            let first = s.spawn(|| {
                let _root = cfg.adopt();
                rename_bus_on(&state, name.clone(), "First".into())
            });
            inside.wait();
            let second = s.spawn(|| {
                let _root = cfg.adopt();
                rename_bus_on(&state, name.clone(), "Second".into())
            });
            first.join().expect("first thread").expect("first rename");
            second
                .join()
                .expect("second thread")
                .expect("second rename");
        });

        assert_rebuilds_do_not_interleave(&backend.bus_ops(), &name);
        let mixer = state.lock_mixer().expect("mixer");
        assert_eq!(
            mixer.buses.get(&name).map(|b| b.label.as_str()),
            Some("Second")
        );
    }
}
