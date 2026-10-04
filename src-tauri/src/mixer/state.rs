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
}
