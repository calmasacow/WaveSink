use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::audio::backend::AudioBackend;
use crate::mixer::state::MixerState;

/// Application state managed by Tauri and shared across commands and the tray.
pub struct AppState {
    pub backend: Arc<dyn AudioBackend>,
    pub mixer: Mutex<MixerState>,
    /// Held for a whole profile load, which takes the mixer lock piecemeal.
    pub profile_switch: Mutex<()>,
    /// Held while a mix node is torn down and rebuilt, so a hotkey or tray
    /// profile switch mid-rebuild can't see it half-built.
    pub bus_rebuild: Mutex<()>,
    pub refresh_gate: Mutex<()>,
    /// Per stream serial, dropped with the stream, so a recycled pid inherits
    /// nothing.
    pub identity_cache: Mutex<HashMap<u64, crate::audio::identity::Identity>>,
    /// Icon and display name per identity key, evicted with the identities.
    pub icon_cache: Mutex<HashMap<String, crate::audio::icons::IconFacts>>,
    /// The same for history rows, which the UI lists every tick.
    pub history_cache: Mutex<HashMap<String, crate::commands::apps::HistoryFacts>>,
}

impl AppState {
    /// Lock the mixer state, mapping poisoning to a command-friendly error.
    /// All command handlers go through this instead of hand-rolled map_errs.
    pub fn lock_mixer(&self) -> Result<std::sync::MutexGuard<'_, MixerState>, String> {
        self.mixer
            .lock()
            .map_err(|_| "mixer state lock poisoned".to_string())
    }

    /// Never taken while the mixer lock is held, so it cannot deadlock.
    pub fn lock_bus_rebuild(&self) -> std::sync::MutexGuard<'_, ()> {
        self.bus_rebuild
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn new(backend: Arc<dyn AudioBackend>) -> Self {
        // Saved assignments are loaded eagerly so auto-routing can enforce
        // them as soon as the sinks exist.
        let routing = crate::routing_model::RoutingModel::load_or_migrate();
        let active_profile = crate::persistence::active::load();
        // Cache the active profile's trigger once so autosave never has to
        // re-read the profile file to preserve it.
        let active_trigger = active_profile
            .as_deref()
            .and_then(|name| crate::persistence::profiles::load(name).ok())
            .and_then(|p| p.trigger_device);
        let now = crate::persistence::unix_now();
        let mut mixer = MixerState {
            assignments: crate::persistence::assignments::Assignments::load(),
            aliases: crate::persistence::aliases::Aliases::load(),
            eq: crate::persistence::eq::ChannelEq::load(),
            routing,
            seen: crate::persistence::seen::SeenApps::load(),
            active_profile,
            active_trigger,
            prefs: crate::persistence::prefs::Prefs::load(),
            seen_saved_at: now,
            ..MixerState::default()
        };
        // One app used to collect several identities depending on how it was
        // launched; fold them together before anything reads them.
        let (seen, assignments, aliases) =
            mixer.merge_exe_identities(&crate::audio::icons::Desktops);
        if seen {
            let _ = mixer.seen.save();
        }
        if assignments {
            let _ = mixer.assignments.save();
        }
        if aliases {
            let _ = mixer.aliases.save();
        }
        if mixer.prune_stale_apps(now) {
            if let Err(e) = mixer.seen.save() {
                eprintln!("wavesink: pruning app history failed: {e}");
            }
        }
        Self {
            backend,
            mixer: Mutex::new(mixer),
            profile_switch: Mutex::new(()),
            bus_rebuild: Mutex::new(()),
            refresh_gate: Mutex::new(()),
            identity_cache: Mutex::new(HashMap::new()),
            icon_cache: Mutex::new(HashMap::new()),
            history_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Best-effort teardown of all virtual sinks: collects errors instead of
    /// aborting on first failure, so one bad unload doesn't strand the rest.
    pub fn teardown_virtual_sinks(&self) -> Vec<String> {
        let names: Vec<String> = self
            .mixer
            .lock()
            .map(|m| m.routing.channels().map(|c| c.id.clone()).collect())
            .unwrap_or_default();
        let mut errors = Vec::new();
        for name in names {
            if let Err(e) = self.backend.destroy_virtual_sink(&name) {
                errors.push(format!("{name}: {e}"));
            }
        }
        if let Ok(mut mixer) = self.mixer.lock() {
            // Persist freshest last-seen timestamps on the way out (the
            // poll only writes on structural changes).
            let _ = mixer.seen.save();
            mixer.reset();
        }
        errors
    }
}
