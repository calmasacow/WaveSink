use std::collections::HashSet;

use crate::audio::types::AppStream;
use crate::persistence::aliases::Aliases;
use crate::persistence::assignments::Assignments;

/// In-memory mixer state: the routing model (the one source of truth for
/// inputs, mixes and routes) plus app assignments, EQ, profiles and prefs.
#[derive(Debug, Default)]
pub struct MixerState {
    /// Inputs, mixes and every input×mix cell, persisted as routing.json.
    pub routing: crate::routing_model::RoutingModel,
    /// True once `init_virtual_devices` has created the sinks.
    pub initialized: bool,
    /// Saved app→channel assignments (persisted to disk + WirePlumber conf).
    pub assignments: Assignments,
    /// User-chosen display names for discovered apps (persisted to disk).
    pub aliases: Aliases,
    /// Per-channel parametric EQ configs (persisted to disk).
    pub eq: crate::persistence::eq::ChannelEq,
    /// Every app identity ever observed (history + ignore list).
    pub seen: crate::persistence::seen::SeenApps,
    /// Unix seconds of the last `seen` write; bounds how stale `last_seen`
    /// gets before the age-based prune trusts it.
    pub seen_saved_at: u64,
    /// Profile changes autosave into this profile (live-bound, not a
    /// snapshot). None = unmanaged state.
    pub active_profile: Option<String>,
    /// Cached trigger device of `active_profile`, so autosave preserves it
    /// without re-reading the profile file on every mutation.
    pub active_trigger: Option<String>,
    /// App preferences (device naming etc.), persisted to disk.
    pub prefs: crate::persistence::prefs::Prefs,
    /// Streams already auto-routed once, by `object.serial` (node ids
    /// recycle); manual re-routing isn't fought.
    pub auto_routed: HashSet<u64>,
}

impl MixerState {
    /// Forget history entries the user never acted on and hasn't seen in a
    /// week; returns true when something changed and should be persisted.
    pub fn prune_stale_apps(&mut self, now: u64) -> bool {
        // Disjoint field borrows: `prune` needs `seen` mutably while the
        // intent test reads the other two.
        let Self {
            seen,
            assignments,
            aliases,
            ..
        } = self;
        seen.prune(
            now,
            crate::persistence::seen::MAX_SEEN_AGE_SECS,
            |prop, value| {
                assignments.sink_for(prop, value).is_some() || aliases.get(prop, value).is_some()
            },
        )
    }

    /// Each stream is considered once, so a manual re-route isn't fought; the
    /// caller applies the returned moves after releasing the lock.
    pub fn plan_auto_routes(&mut self, streams: &[AppStream]) -> Vec<(u32, String, String)> {
        // Enforce only once the virtual sinks exist, or streams would be
        // marked handled while their target can't be moved to yet.
        if !self.initialized {
            return Vec::new();
        }
        let mut planned = Vec::new();
        for stream in streams {
            if !stream.settled || self.auto_routed.contains(&stream.serial) {
                continue;
            }
            if let Some(target) = self.rule_for(stream) {
                if stream.assigned_sink.as_deref() != Some(target.as_str()) {
                    planned.push((stream.index, target, stream.app_name.clone()));
                }
            }
            self.auto_routed.insert(stream.serial);
        }
        let live: HashSet<u64> = streams.iter().map(|s| s.serial).collect();
        self.auto_routed.retain(|serial| live.contains(serial));
        planned
    }

    /// A process identity's own rule wins; only an identity with no trusted
    /// process falls back to a rule on the stream's own props.
    fn rule_for(&self, stream: &AppStream) -> Option<String> {
        self.assignments
            .sink_for(&stream.match_prop, &stream.match_value)
            .or_else(|| {
                if crate::audio::identity::is_process_prop(&stream.match_prop) {
                    return None;
                }
                crate::audio::identity::legacy_matchers(&stream.props)
                    .iter()
                    .find_map(|(prop, value)| self.assignments.sink_for(prop, value))
            })
            .map(str::to_string)
    }

    /// Fold history rows, assignments and aliases keyed on a plain
    /// executable into the desktop app it now resolves to (see
    /// `identity::desktop_for_exe`). Earlier versions keyed one app on its
    /// exe or its desktop id depending on how it was launched, so it showed
    /// up several times. A desktop-keyed assignment or alias wins over the
    /// exe one; history keeps the latest sighting. Returns which stores
    /// changed: (seen, assignments, aliases).
    pub fn merge_exe_identities(
        &mut self,
        desktops: &dyn crate::audio::identity::DesktopDb,
    ) -> (bool, bool, bool) {
        use crate::audio::identity::{desktop_for_exe, PROP_DESKTOP, PROP_EXE};
        use crate::persistence::assignments::identity_key;
        let target = |prop: &str, value: &str| {
            (prop == PROP_EXE)
                .then(|| desktop_for_exe(desktops, value))
                .flatten()
        };

        let mut seen_changed = false;
        let mut i = 0;
        while i < self.seen.apps.len() {
            let entry = &self.seen.apps[i];
            let Some((id, name)) = target(&entry.match_prop, &entry.match_value) else {
                i += 1;
                continue;
            };
            seen_changed = true;
            let source = self.seen.apps.remove(i);
            match self
                .seen
                .apps
                .iter_mut()
                .find(|e| e.match_prop == PROP_DESKTOP && e.match_value == id)
            {
                Some(existing) => {
                    if source.last_seen > existing.last_seen {
                        existing.last_seen = source.last_seen;
                    }
                    existing.ignored |= source.ignored;
                    if existing.icon_path.is_none() {
                        existing.icon_path = source.icon_path;
                    }
                }
                None => self.seen.apps.insert(
                    i,
                    crate::persistence::seen::SeenEntry {
                        match_prop: PROP_DESKTOP.to_string(),
                        match_value: id,
                        display_name: name,
                        ..source
                    },
                ),
            }
        }

        // Rename exe-keyed rules, then keep one rule per identity: a rule that
        // was already desktop-keyed wins over a renamed one (stable sort).
        let mut renamed_keys: Vec<(String, String)> = Vec::new();
        let mut rules: Vec<(bool, crate::persistence::assignments::Assignment)> =
            std::mem::take(&mut self.assignments.assignments)
                .into_iter()
                .map(|mut a| {
                    let moved = target(&a.match_prop, &a.match_value).map(|(id, _)| {
                        renamed_keys.push((
                            identity_key(&a.match_prop, &a.match_value),
                            identity_key(PROP_DESKTOP, &id),
                        ));
                        a.match_prop = PROP_DESKTOP.to_string();
                        a.match_value = id;
                    });
                    (moved.is_some(), a)
                })
                .collect();
        let assignments_changed = !renamed_keys.is_empty();
        rules.sort_by_key(|(moved, _)| *moved);
        let mut out: Vec<crate::persistence::assignments::Assignment> = Vec::new();
        for (_, mut a) in rules {
            if out
                .iter()
                .any(|o| o.match_prop == a.match_prop && o.match_value == a.match_value)
            {
                continue;
            }
            for key in &mut a.adopted_by {
                if let Some((_, to)) = renamed_keys.iter().find(|(from, _)| from == key) {
                    *key = to.clone();
                }
            }
            out.push(a);
        }
        self.assignments.assignments = out;

        let mut aliases: Vec<(bool, crate::persistence::aliases::AliasEntry)> =
            std::mem::take(&mut self.aliases.aliases)
                .into_iter()
                .map(|mut alias| {
                    let moved = target(&alias.match_prop, &alias.match_value).map(|(id, _)| {
                        alias.match_prop = PROP_DESKTOP.to_string();
                        alias.match_value = id;
                    });
                    (moved.is_some(), alias)
                })
                .collect();
        let aliases_changed = aliases.iter().any(|(moved, _)| *moved);
        aliases.sort_by_key(|(moved, _)| *moved);
        let mut out: Vec<crate::persistence::aliases::AliasEntry> = Vec::new();
        for (_, alias) in aliases {
            if !out
                .iter()
                .any(|o| o.match_prop == alias.match_prop && o.match_value == alias.match_value)
            {
                out.push(alias);
            }
        }
        self.aliases.aliases = out;

        (seen_changed, assignments_changed, aliases_changed)
    }

    pub fn reset(&mut self) {
        self.initialized = false;
    }

    /// Test setup: the classic four channels in one mix, sinks "created".
    #[cfg(test)]
    pub fn init_test_defaults(&mut self) {
        self.routing = crate::routing_model::RoutingModel::from_legacy(
            &crate::persistence::channels::Channels::default(),
            &crate::persistence::buses::Buses::default(),
            &Default::default(),
        );
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_stale_apps_exempts_assigned_and_aliased() {
        const DAY: u64 = 24 * 60 * 60;
        let now = 100 * DAY;
        let old = now - 30 * DAY;
        let mut state = MixerState::default();
        for value in ["plain", "assigned", "aliased"] {
            state
                .seen
                .upsert("application.name", value, value, None, None, old);
        }
        state
            .assignments
            .set("application.name", "assigned", "sink_game");
        state.aliases.set("application.name", "aliased", "My App");

        assert!(state.prune_stale_apps(now));
        assert!(state.seen.get("application.name", "plain").is_none());
        assert!(state.seen.get("application.name", "assigned").is_some());
        assert!(state.seen.get("application.name", "aliased").is_some());
    }

    fn stream(index: u32, serial: u64, value: &str, on: Option<&str>) -> AppStream {
        AppStream {
            index,
            serial,
            app_name: value.to_string(),
            match_prop: "application.name".into(),
            match_value: value.into(),
            alias: None,
            icon_name: None,
            icon_path: None,
            pid: None,
            assigned_sink: on.map(str::to_string),
            volume_percent: 100,
            muted: false,
            active: true,
            props: Default::default(),
            settled: true,
        }
    }

    #[test]
    fn auto_route_plans_once_and_respects_a_manual_move() {
        let mut state = MixerState::default();
        state.init_test_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");

        // First sight: planned, and marked handled.
        let planned = state.plan_auto_routes(&[stream(7, 100, "Firefox", None)]);
        assert_eq!(
            planned,
            vec![(7, "sink_game".to_string(), "Firefox".to_string())]
        );

        // Seen again, moved elsewhere by hand: not fought.
        let planned = state.plan_auto_routes(&[stream(7, 100, "Firefox", Some("sink_chat"))]);
        assert!(planned.is_empty());
    }

    #[test]
    fn auto_route_skips_a_stream_already_on_target() {
        let mut state = MixerState::default();
        state.init_test_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        assert!(state
            .plan_auto_routes(&[stream(7, 100, "Firefox", Some("sink_game"))])
            .is_empty());
    }

    #[test]
    fn auto_route_waits_for_the_sinks_to_exist() {
        let mut state = MixerState::default();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        // Nothing planned, and nothing marked handled - marking here would
        // strand the stream once the sinks arrive.
        assert!(state
            .plan_auto_routes(&[stream(7, 100, "Firefox", None)])
            .is_empty());
        assert!(state.auto_routed.is_empty());
    }

    #[test]
    fn auto_route_reroutes_a_restarted_stream_on_a_recycled_node_id() {
        let mut state = MixerState::default();
        state.init_test_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        assert_eq!(
            state
                .plan_auto_routes(&[stream(7, 100, "Firefox", None)])
                .len(),
            1
        );

        // The app reopened its stream and PipeWire reused the node id;
        // serials never repeat, so the new stream is still routed.
        let planned = state.plan_auto_routes(&[stream(7, 101, "Firefox", None)]);
        assert_eq!(planned.len(), 1);
    }

    #[test]
    fn auto_route_leaves_an_unsettled_stream_for_the_next_tick() {
        let mut state = MixerState::default();
        state.init_test_defaults();
        state
            .assignments
            .set("application.name", "Firefox", "sink_game");
        let mut early = stream(7, 100, "Firefox", None);
        early.settled = false;
        assert!(state.plan_auto_routes(&[early]).is_empty());
        assert!(state.auto_routed.is_empty(), "not ledgered while unsettled");
        assert_eq!(
            state
                .plan_auto_routes(&[stream(7, 100, "Firefox", None)])
                .len(),
            1
        );
    }

    #[test]
    fn auto_route_still_honours_a_rule_keyed_on_the_streams_own_props() {
        // Bug shape: a rule keyed on the stream's own media.name must still
        // apply.
        let mut state = MixerState::default();
        state.init_test_defaults();
        state
            .assignments
            .set("media.name", "audio-src", "sink_music");
        let mut s = stream(3, 30, "Unknown", None);
        s.props
            .insert("media.name".to_string(), "audio-src".to_string());
        assert_eq!(
            state.plan_auto_routes(&[s]),
            vec![(3, "sink_music".to_string(), "Unknown".to_string())]
        );
    }

    #[test]
    fn a_cleared_rule_on_a_process_identity_stays_cleared() {
        // Bug shape: a cleared rule must not be revived by its adopted legacy
        // rule.
        let mut state = MixerState::default();
        state.init_test_defaults();
        state
            .assignments
            .set("application.name", "Discord", "sink_voice");
        let mut s = stream(3, 30, "Discord", None);
        s.match_prop = "process.exe".into();
        s.match_value = "discord".into();
        s.props
            .insert("application.name".to_string(), "Discord".to_string());
        assert!(state.plan_auto_routes(&[s]).is_empty());
    }

    #[test]
    fn auto_route_ledger_forgets_dead_streams() {
        let mut state = MixerState::default();
        state.init_test_defaults();
        state.plan_auto_routes(&[stream(1, 10, "A", None), stream(2, 11, "B", None)]);
        assert_eq!(state.auto_routed.len(), 2);
        state.plan_auto_routes(&[stream(1, 10, "A", None)]);
        assert_eq!(state.auto_routed, HashSet::from([10]));
    }

    struct OneDesktop;
    impl crate::audio::identity::DesktopDb for OneDesktop {
        fn name_by_id(&self, _: &str) -> Option<String> {
            None
        }
        fn entry_for_exec(&self, _: &[String], _: &str) -> Option<(String, String)> {
            None
        }
        fn entry_by_exec(&self, exe: &str) -> Option<(String, String)> {
            (exe == "obs").then(|| ("com.obsproject.studio".into(), "OBS Studio".into()))
        }
    }

    // Bug shape: OBS from a launcher was keyed on its desktop id, from a
    // terminal on its exe, so the Apps screen listed it twice.
    #[test]
    fn exe_keyed_identities_fold_into_their_desktop_app() {
        use crate::persistence::seen::SeenEntry;
        let seen = |prop: &str, value: &str, last_seen| SeenEntry {
            match_prop: prop.into(),
            match_value: value.into(),
            display_name: "OBS Studio".into(),
            icon_name: None,
            icon_path: None,
            last_seen,
            ignored: false,
        };
        let mut state = MixerState::default();
        state.seen.apps = vec![
            seen("desktop.id", "com.obsproject.studio", 10),
            seen("process.exe", "obs", 20),
            seen("process.exe", "obs-browser-page", 5),
        ];
        state
            .assignments
            .set("desktop.id", "com.obsproject.studio", "sink_obs_monitor");
        state.assignments.set("process.exe", "obs", "sink_game");
        state
            .assignments
            .set("process.exe", "obs-browser-page", "sink_browser");
        state.aliases.set("process.exe", "obs", "OBS");

        let (s, a, al) = state.merge_exe_identities(&OneDesktop);
        assert!(s && a && al);
        let obs: Vec<_> = state
            .seen
            .apps
            .iter()
            .filter(|e| e.match_value == "com.obsproject.studio")
            .collect();
        assert_eq!(obs.len(), 1, "one row for OBS");
        assert_eq!(obs[0].last_seen, 20, "latest sighting kept");
        assert_eq!(
            state
                .assignments
                .sink_for("desktop.id", "com.obsproject.studio"),
            Some("sink_obs_monitor"),
            "the desktop-keyed rule wins"
        );
        assert!(state.assignments.sink_for("process.exe", "obs").is_none());
        // An exe no desktop entry runs is left alone.
        assert_eq!(
            state
                .assignments
                .sink_for("process.exe", "obs-browser-page"),
            Some("sink_browser")
        );
        assert_eq!(
            state.aliases.get("desktop.id", "com.obsproject.studio"),
            Some("OBS")
        );
        assert_eq!(
            state.merge_exe_identities(&OneDesktop),
            (false, false, false)
        );
    }
}
