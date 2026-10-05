use tauri::State;

use crate::commands::routing::MAX_VOLUME;
use crate::state::AppState;

/// Apply a change to one mix's definition and persist it.
fn edit_mix(
    state: &AppState,
    name: &str,
    edit: impl FnOnce(&mut crate::routing_model::MixDef),
) -> Result<(), String> {
    let mut mixer = state.lock_mixer()?;
    edit(mixer.routing.mix_mut(name).map_err(|e| e.to_string())?);
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Create a new mix. Recorders see it under `label`. It starts empty: tick
/// the cells of the inputs it should carry.
#[tauri::command]
pub fn add_bus(state: State<'_, AppState>, label: String) -> Result<(), String> {
    let (mix, prefs) = {
        let mut mixer = state.lock_mixer()?;
        let mix = mixer.routing.add_mix(&label).map_err(|e| e.to_string())?;
        (mix, mixer.prefs.clone())
    };
    if let Err(e) = state
        .backend
        .create_bus(&mix.id, &prefs.decorate(&mix.label))
    {
        let mut mixer = state.lock_mixer()?;
        let _ = mixer.routing.remove_mix(&mix.id);
        return Err(e.to_string());
    }
    let mixer = state.lock_mixer()?;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Rename a mix by recreating its node - the node name itself stays stable,
/// so OBS configs keep working and capture re-attaches automatically.
#[tauri::command]
pub fn rename_bus(state: State<'_, AppState>, name: String, label: String) -> Result<(), String> {
    rename_bus_on(&state, name, label)
}

pub fn rename_bus_on(state: &AppState, name: String, label: String) -> Result<(), String> {
    let _rebuild = state.lock_bus_rebuild();
    let (model, prefs) = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .routing
            .rename_mix(&name, &label)
            .map_err(|e| e.to_string())?;
        mixer.routing.save().map_err(|e| e.to_string())?;
        crate::commands::profiles::autosave_active(&mixer);
        (mixer.routing.clone(), mixer.prefs.clone())
    };
    let mix = model
        .mix(&name)
        .cloned()
        .ok_or_else(|| "unknown mix".to_string())?;
    state
        .backend
        .destroy_bus(&name)
        .map_err(|e| e.to_string())?;
    // The node is fresh: give it back everything it carried.
    crate::commands::graph::bring_up_mix(state, &model, &mix, &prefs);
    Ok(())
}

#[tauri::command]
pub fn set_bus_icon(state: State<'_, AppState>, name: String, icon: String) -> Result<(), String> {
    edit_mix(&state, &name, |mix| mix.icon = Some(icon))
}

#[tauri::command]
pub fn set_bus_icon_color(
    state: State<'_, AppState>,
    name: String,
    icon_color: String,
) -> Result<(), String> {
    edit_mix(&state, &name, |mix| mix.icon_color = Some(icon_color))
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
    if state.lock_mixer()?.routing.mix(&name).is_none() {
        return Err(format!("unknown mix: {name}"));
    }
    state
        .backend
        .destroy_bus(&name)
        .map_err(|e| e.to_string())?;
    let mut mixer = state.lock_mixer()?;
    mixer.routing.remove_mix(&name).map_err(|e| e.to_string())?;
    mixer.routing.save().map_err(|e| e.to_string())?;
    crate::commands::profiles::autosave_active(&mixer);
    Ok(())
}

/// Set a mix's level (0-100%): what recorders and its outputs hear. Also the
/// target of `wavesink --set-mix-volume` from the Omarchy audio panel.
#[tauri::command]
pub fn set_bus_volume(state: State<'_, AppState>, name: String, volume: u8) -> Result<(), String> {
    set_bus_volume_on(&state, &name, volume)
}

pub fn set_bus_volume_on(state: &AppState, name: &str, volume: u8) -> Result<(), String> {
    if state.lock_mixer()?.routing.mix(name).is_none() {
        return Err(format!("unknown mix: {name}"));
    }
    let volume = volume.min(MAX_VOLUME);
    state
        .backend
        .set_sink_volume(name, volume)
        .map_err(|e| e.to_string())?;
    edit_mix(state, name, |mix| mix.volume_percent = volume)
}

/// Mute or unmute a mix for recorders and its outputs. Persisted.
#[tauri::command]
pub fn set_bus_mute(state: State<'_, AppState>, name: String, muted: bool) -> Result<(), String> {
    set_bus_mute_on(&state, &name, muted)
}

pub fn set_bus_mute_on(state: &AppState, name: &str, muted: bool) -> Result<(), String> {
    if state.lock_mixer()?.routing.mix(name).is_none() {
        return Err(format!("unknown mix: {name}"));
    }
    state
        .backend
        .set_sink_mute(name, muted)
        .map_err(|e| e.to_string())?;
    edit_mix(state, name, |mix| mix.muted = muted)
}

#[cfg(test)]
mod tests {
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
            mixer.initialized = true;
            mixer.routing.add_mix("Solo").expect("add mix").id
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
            mixer.routing.mix(&name).map(|m| m.label.as_str()),
            Some("Second")
        );
    }
}
