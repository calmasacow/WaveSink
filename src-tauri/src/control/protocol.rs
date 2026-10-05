//! The control protocol: newline-delimited JSON over the control socket.
//!
//! A request is `{"id": 1, "method": "set_mix_volume", "params": {...}}` and
//! is answered `{"id": 1, "result": ...}` or `{"id": 1, "error": "..."}`.
//! A subscribed client is also pushed `{"event": "state", "data": Snapshot}`
//! whenever anything it can see changes, from here or from the UI, and
//! `{"event": "levels", "data": Levels}` at the meter rate.
//!
//! Every mutation goes through the same `_on` command functions the UI
//! invokes, so the socket can never do what the mixer would refuse.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::routing::MAX_VOLUME;
use crate::routing_model::{InputKind, RouteCell, RoutingModel};
use crate::state::AppState;

/// Bumped on any incompatible change to requests, the snapshot or events.
pub const PROTOCOL_VERSION: u32 = 1;

/// Longest request line accepted; anything longer closes the connection.
pub const MAX_LINE: u64 = 64 * 1024;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Call {
    Hello {},
    GetState {},
    Subscribe {
        #[serde(default = "yes")]
        state: bool,
        #[serde(default)]
        levels: bool,
    },
    SetInputVolume {
        input: String,
        volume: u8,
    },
    AdjustInputVolume {
        input: String,
        delta: i16,
    },
    /// `muted` omitted toggles.
    SetInputMute {
        input: String,
        #[serde(default)]
        muted: Option<bool>,
    },
    SetMixVolume {
        mix: String,
        volume: u8,
    },
    AdjustMixVolume {
        mix: String,
        delta: i16,
    },
    /// `muted` omitted toggles.
    SetMixMute {
        mix: String,
        #[serde(default)]
        muted: Option<bool>,
    },
    /// Fields omitted keep their current value.
    SetRoute {
        input: String,
        mix: String,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        send: Option<u8>,
        #[serde(default)]
        muted: Option<bool>,
    },
    ToggleRoute {
        input: String,
        mix: String,
        #[serde(default)]
        field: RouteField,
    },
    AdjustRouteSend {
        input: String,
        mix: String,
        delta: i16,
    },
    ToggleSolo {
        input: String,
    },
    LoadProfile {
        name: String,
    },
    /// +1 for the next profile, -1 for the previous, wrapping.
    CycleProfile {
        #[serde(default = "forward")]
        direction: i32,
    },
    ShowWindow {},
}

fn yes() -> bool {
    true
}

fn forward() -> i32 {
    1
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RouteField {
    #[default]
    Enabled,
    Muted,
}

/// One parsed request line. `id` is echoed back so a client can match
/// replies; a request without one still gets a reply, with `id: null`.
#[derive(Debug)]
pub struct Request {
    pub id: Value,
    pub call: Result<Call, String>,
}

pub fn parse(line: &str) -> Request {
    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Request {
                id: Value::Null,
                call: Err(format!("invalid JSON: {e}")),
            }
        }
    };
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return Request {
            id,
            call: Err("missing method".into()),
        };
    };
    let params = match value.get("params") {
        None | Some(Value::Null) => Value::Object(Default::default()),
        Some(p) => p.clone(),
    };
    let call = serde_json::from_value(serde_json::json!({ "method": method, "params": params }))
        .map_err(|e| format!("{method}: {e}"));
    Request { id, call }
}

pub fn reply(id: &Value, result: Result<Value, String>) -> String {
    let body = match result {
        Ok(result) => serde_json::json!({ "id": id, "result": result }),
        Err(error) => serde_json::json!({ "id": id, "error": error }),
    };
    body.to_string()
}

pub fn event(name: &str, data: &impl Serialize) -> String {
    serde_json::json!({ "event": name, "data": data }).to_string()
}

/// What a successful call changed, so the server can tell the UI.
#[derive(Debug, PartialEq)]
pub enum Effect {
    None,
    /// Levels, mutes, routes or solo.
    Mixer,
    /// A profile was loaded; the whole UI and the tray must resync.
    Profile(String),
    ShowWindow,
    Subscribe {
        state: bool,
        levels: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CellView {
    pub enabled: bool,
    pub send: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputView {
    pub id: String,
    pub label: String,
    pub kind: &'static str,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub volume: u8,
    pub muted: bool,
    /// Every mix, routed or not: mix id -> cell.
    pub routes: BTreeMap<String, CellView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixView {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub volume: u8,
    pub muted: bool,
}

/// Everything a control surface shows, in matrix order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub protocol: u32,
    pub inputs: Vec<InputView>,
    pub mixes: Vec<MixView>,
    pub solo: Option<String>,
    pub profiles: Vec<String>,
    pub active_profile: Option<String>,
}

impl Snapshot {
    pub fn build(model: &RoutingModel, profiles: Vec<String>, active: Option<String>) -> Self {
        let mut inputs: Vec<_> = model.inputs.iter().collect();
        inputs.sort_by_key(|i| i.order);
        let mut mixes: Vec<_> = model.mixes.iter().collect();
        mixes.sort_by_key(|m| m.order);
        Self {
            protocol: PROTOCOL_VERSION,
            inputs: inputs
                .iter()
                .map(|input| InputView {
                    id: input.id.clone(),
                    label: input.label.clone(),
                    kind: match input.kind {
                        InputKind::Software => "software",
                        InputKind::Hardware => "hardware",
                    },
                    icon: input.icon.clone(),
                    icon_color: input.icon_color.clone(),
                    volume: input.volume_percent,
                    muted: input.muted,
                    routes: mixes
                        .iter()
                        .map(|mix| {
                            let cell = model.cell(&input.id, &mix.id);
                            (
                                mix.id.clone(),
                                CellView {
                                    enabled: cell.enabled,
                                    send: cell.send_percent,
                                    muted: cell.muted,
                                },
                            )
                        })
                        .collect(),
                })
                .collect(),
            mixes: mixes
                .iter()
                .map(|mix| MixView {
                    id: mix.id.clone(),
                    label: mix.label.clone(),
                    icon: mix.icon.clone(),
                    icon_color: mix.icon_color.clone(),
                    volume: mix.volume_percent,
                    muted: mix.muted,
                })
                .collect(),
            solo: model.solo.as_ref().map(|s| s.input.clone()),
            profiles,
            active_profile: active,
        }
    }

    /// The live snapshot. Reads the profile directory; the broadcaster
    /// passes its cached list instead.
    pub fn current(state: &AppState, profiles: Option<Vec<String>>) -> Result<Self, String> {
        let profiles = match profiles {
            Some(p) => p,
            None => profile_names()?,
        };
        let mixer = state.lock_mixer()?;
        Ok(Self::build(
            &mixer.routing,
            profiles,
            mixer.active_profile.clone(),
        ))
    }
}

pub fn profile_names() -> Result<Vec<String>, String> {
    Ok(crate::persistence::profiles::list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| p.name)
        .collect())
}

/// Which level-store key meters an input or mix, the same choice the
/// routing table makes: channels on their sink, hardware inputs on what their
/// mixes receive (the processed stream while Audio FX is on).
pub fn meter_keys(model: &RoutingModel) -> Vec<MeterKey> {
    let inputs = model.inputs.iter().map(|input| MeterKey {
        mix: false,
        id: input.id.clone(),
        key: match input.kind {
            InputKind::Software => input.id.clone(),
            InputKind::Hardware if input.fx.is_active() => {
                format!("{}{}", crate::audio::pw_native::FX_LEVEL_PREFIX, input.id)
            }
            InputKind::Hardware => input.source_name.clone(),
        },
    });
    let mixes = model.mixes.iter().map(|mix| MeterKey {
        mix: true,
        id: mix.id.clone(),
        key: mix.id.clone(),
    });
    inputs.chain(mixes).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeterKey {
    pub mix: bool,
    pub id: String,
    pub key: String,
}

/// Peak (0..1, linear, the louder side) per input and per mix.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Levels {
    pub inputs: BTreeMap<String, f32>,
    pub mixes: BTreeMap<String, f32>,
}

impl Levels {
    pub fn from_raw(keys: &[MeterKey], raw: &std::collections::HashMap<String, [f32; 2]>) -> Self {
        let mut levels = Self::default();
        for key in keys {
            let peak = raw.get(&key.key).map(|[l, r]| l.max(*r)).unwrap_or(0.0);
            let side = if key.mix {
                &mut levels.mixes
            } else {
                &mut levels.inputs
            };
            side.insert(key.id.clone(), peak);
        }
        levels
    }
}

fn step(current: u8, delta: i16) -> u8 {
    (i16::from(current) + delta).clamp(0, i16::from(MAX_VOLUME)) as u8
}

fn input_level(state: &AppState, input: &str) -> Result<(InputKind, u8, bool), String> {
    let mixer = state.lock_mixer()?;
    let def = mixer
        .routing
        .input(input)
        .ok_or_else(|| format!("unknown input {input}"))?;
    Ok((def.kind.clone(), def.volume_percent, def.muted))
}

/// Software inputs are channel sinks; hardware inputs are matrix sources.
/// The UI drives them through different commands, and so does this.
fn set_input(state: &AppState, input: &str, volume: u8, muted: bool) -> Result<(), String> {
    let (kind, old_volume, old_muted) = input_level(state, input)?;
    match kind {
        InputKind::Software => {
            if volume != old_volume {
                crate::commands::routing::set_channel_volume_on(state, input, volume)?;
            }
            if muted != old_muted {
                crate::commands::routing::set_channel_mute_on(state, input, muted)?;
            }
            Ok(())
        }
        InputKind::Hardware => {
            crate::commands::matrix::set_input_level_on(state, input, volume, muted)
        }
    }
}

fn mix_level(state: &AppState, mix: &str) -> Result<(u8, bool), String> {
    let mixer = state.lock_mixer()?;
    let def = mixer
        .routing
        .mix(mix)
        .ok_or_else(|| format!("unknown mix {mix}"))?;
    Ok((def.volume_percent, def.muted))
}

fn cell(state: &AppState, input: &str, mix: &str) -> Result<RouteCell, String> {
    let mixer = state.lock_mixer()?;
    if mixer.routing.input(input).is_none() {
        return Err(format!("unknown input {input}"));
    }
    if mixer.routing.mix(mix).is_none() {
        return Err(format!("unknown mix {mix}"));
    }
    Ok(mixer.routing.cell(input, mix))
}

/// Run one call. Subscribing and showing the window are the server's job;
/// they come back as effects.
pub fn dispatch(state: &AppState, call: Call) -> Result<(Value, Effect), String> {
    let done = |effect| Ok((Value::Null, effect));
    match call {
        Call::Hello {} => Ok((
            serde_json::json!({
                "app": "wavesink",
                "version": env!("CARGO_PKG_VERSION"),
                "protocol": PROTOCOL_VERSION,
            }),
            Effect::None,
        )),
        Call::GetState {} => {
            let snapshot = Snapshot::current(state, None)?;
            Ok((
                serde_json::to_value(snapshot).map_err(|e| e.to_string())?,
                Effect::None,
            ))
        }
        Call::Subscribe { state, levels } => done(Effect::Subscribe { state, levels }),
        Call::SetInputVolume { input, volume } => {
            let (_, _, muted) = input_level(state, &input)?;
            set_input(state, &input, volume.min(MAX_VOLUME), muted)?;
            done(Effect::Mixer)
        }
        Call::AdjustInputVolume { input, delta } => {
            let (_, volume, muted) = input_level(state, &input)?;
            set_input(state, &input, step(volume, delta), muted)?;
            done(Effect::Mixer)
        }
        Call::SetInputMute { input, muted } => {
            let (_, volume, current) = input_level(state, &input)?;
            set_input(state, &input, volume, muted.unwrap_or(!current))?;
            done(Effect::Mixer)
        }
        Call::SetMixVolume { mix, volume } => {
            crate::commands::buses::set_bus_volume_on(state, &mix, volume)?;
            done(Effect::Mixer)
        }
        Call::AdjustMixVolume { mix, delta } => {
            let (volume, _) = mix_level(state, &mix)?;
            crate::commands::buses::set_bus_volume_on(state, &mix, step(volume, delta))?;
            done(Effect::Mixer)
        }
        Call::SetMixMute { mix, muted } => {
            let (_, current) = mix_level(state, &mix)?;
            crate::commands::buses::set_bus_mute_on(state, &mix, muted.unwrap_or(!current))?;
            done(Effect::Mixer)
        }
        Call::SetRoute {
            input,
            mix,
            enabled,
            send,
            muted,
        } => {
            let current = cell(state, &input, &mix)?;
            let next = RouteCell {
                enabled: enabled.unwrap_or(current.enabled),
                send_percent: send.unwrap_or(current.send_percent).min(MAX_VOLUME),
                muted: muted.unwrap_or(current.muted),
            };
            crate::commands::matrix::set_route_cell_on(state, &input, &mix, next)?;
            done(Effect::Mixer)
        }
        Call::ToggleRoute { input, mix, field } => {
            let mut next = cell(state, &input, &mix)?;
            match field {
                RouteField::Enabled => next.enabled = !next.enabled,
                RouteField::Muted => next.muted = !next.muted,
            }
            crate::commands::matrix::set_route_cell_on(state, &input, &mix, next)?;
            done(Effect::Mixer)
        }
        Call::AdjustRouteSend { input, mix, delta } => {
            let mut next = cell(state, &input, &mix)?;
            next.send_percent = step(next.send_percent, delta);
            crate::commands::matrix::set_route_cell_on(state, &input, &mix, next)?;
            done(Effect::Mixer)
        }
        Call::ToggleSolo { input } => {
            crate::commands::matrix::toggle_solo_on(state, &input)?;
            done(Effect::Mixer)
        }
        Call::LoadProfile { name } => {
            crate::commands::profiles::load_profile_on(state, name.clone())?;
            done(Effect::Profile(name))
        }
        Call::CycleProfile { direction } => {
            let names = profile_names()?;
            if names.is_empty() {
                return done(Effect::None);
            }
            let active = state.lock_mixer()?.active_profile.clone();
            let name = crate::hotkeys::next_profile(
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
                active.as_deref(),
                direction.signum(),
            )
            .to_string();
            crate::commands::profiles::load_profile_on(state, name.clone())?;
            done(Effect::Profile(name))
        }
        Call::ShowWindow {} => done(Effect::ShowWindow),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::mock::{Call as Backend, MockBackend};
    use crate::persistence::testing::TempConfig;
    use std::sync::Arc;

    fn app(backend: Arc<MockBackend>) -> AppState {
        let state = AppState::new(backend);
        state.lock_mixer().expect("mixer").init_test_defaults();
        state
    }

    fn ids(state: &AppState) -> (String, String) {
        let mixer = state.lock_mixer().expect("mixer");
        (
            mixer.routing.inputs[0].id.clone(),
            mixer.routing.mixes[0].id.clone(),
        )
    }

    fn run(state: &AppState, line: &str) -> Result<(Value, Effect), String> {
        dispatch(state, parse(line).call?)
    }

    #[test]
    fn parses_requests_with_and_without_params() {
        let req = parse(r#"{"id":4,"method":"get_state"}"#);
        assert_eq!(req.id, serde_json::json!(4));
        assert_eq!(req.call, Ok(Call::GetState {}));

        let req = parse(r#"{"method":"set_mix_mute","params":{"mix":"m"}}"#);
        assert_eq!(req.id, Value::Null);
        assert_eq!(
            req.call,
            Ok(Call::SetMixMute {
                mix: "m".into(),
                muted: None
            })
        );

        let req = parse(r#"{"id":"a","method":"subscribe","params":{"levels":true}}"#);
        assert_eq!(
            req.call,
            Ok(Call::Subscribe {
                state: true,
                levels: true
            })
        );
    }

    #[test]
    fn rejects_bad_requests_without_panicking() {
        assert!(parse("not json").call.is_err());
        assert!(parse(r#"{"id":1}"#).call.is_err());
        assert!(parse(r#"{"id":1,"method":"rm_rf"}"#).call.is_err());
        assert!(
            parse(r#"{"id":1,"method":"set_mix_volume","params":{"mix":"m"}}"#)
                .call
                .is_err()
        );
        // u8 refuses out-of-range levels before anything runs.
        assert!(
            parse(r#"{"method":"set_mix_volume","params":{"mix":"m","volume":900}}"#)
                .call
                .is_err()
        );
    }

    #[test]
    fn replies_echo_the_id() {
        let ok: Value =
            serde_json::from_str(&reply(&serde_json::json!(9), Ok(Value::Null))).expect("json");
        assert_eq!(ok, serde_json::json!({"id": 9, "result": null}));
        let err: Value =
            serde_json::from_str(&reply(&Value::Null, Err("nope".into()))).expect("json");
        assert_eq!(err, serde_json::json!({"id": null, "error": "nope"}));
    }

    #[test]
    fn snapshot_lists_every_cell_in_matrix_order() {
        let _cfg = TempConfig::new("control-snapshot");
        let state = app(Arc::new(MockBackend::default()));
        let (input, mix) = ids(&state);
        run(
            &state,
            &format!(
                r#"{{"method":"set_route","params":{{"input":"{input}","mix":"{mix}","enabled":true,"send":40}}}}"#
            ),
        )
        .expect("set route");

        let snap = Snapshot::current(&state, Some(vec![])).expect("snapshot");
        let mixer = state.lock_mixer().expect("mixer");
        assert_eq!(snap.inputs.len(), mixer.routing.inputs.len());
        for view in &snap.inputs {
            assert_eq!(view.routes.len(), mixer.routing.mixes.len());
        }
        let routed = &snap.inputs[0].routes[&mix];
        assert_eq!(
            routed,
            &CellView {
                enabled: true,
                send: 40,
                muted: false
            }
        );
        assert!(snap.inputs.windows(2).all(|w| {
            let order = |id: &str| mixer.routing.input(id).expect("input").order;
            order(&w[0].id) < order(&w[1].id)
        }));
    }

    #[test]
    fn adjusting_a_mix_clamps_to_unity_and_reaches_the_backend() {
        let _cfg = TempConfig::new("control-mix");
        let backend = Arc::new(MockBackend::default());
        let state = app(backend.clone());
        let (_, mix) = ids(&state);
        run(
            &state,
            &format!(r#"{{"method":"set_mix_volume","params":{{"mix":"{mix}","volume":95}}}}"#),
        )
        .expect("set");
        let (_, effect) = run(
            &state,
            &format!(r#"{{"method":"adjust_mix_volume","params":{{"mix":"{mix}","delta":20}}}}"#),
        )
        .expect("adjust");
        assert_eq!(effect, Effect::Mixer);
        assert_eq!(mix_level(&state, &mix).expect("mix").0, 100);
        run(
            &state,
            &format!(r#"{{"method":"adjust_mix_volume","params":{{"mix":"{mix}","delta":-300}}}}"#),
        )
        .expect("adjust down");
        assert_eq!(mix_level(&state, &mix).expect("mix").0, 0);
        assert!(backend.calls().contains(&Backend::Other("set_sink_volume")));
    }

    #[test]
    fn mute_without_a_value_toggles() {
        let _cfg = TempConfig::new("control-mute");
        let state = app(Arc::new(MockBackend::default()));
        let (input, mix) = ids(&state);
        let toggle_input =
            format!(r#"{{"method":"set_input_mute","params":{{"input":"{input}"}}}}"#);
        run(&state, &toggle_input).expect("mute");
        assert!(input_level(&state, &input).expect("input").2);
        run(&state, &toggle_input).expect("unmute");
        assert!(!input_level(&state, &input).expect("input").2);

        let toggle_mix = format!(r#"{{"method":"set_mix_mute","params":{{"mix":"{mix}"}}}}"#);
        run(&state, &toggle_mix).expect("mute mix");
        assert!(mix_level(&state, &mix).expect("mix").1);
    }

    #[test]
    fn route_toggles_touch_only_their_own_cell_field() {
        let _cfg = TempConfig::new("control-route");
        let state = app(Arc::new(MockBackend::default()));
        let (input, mix) = ids(&state);
        let before = cell(&state, &input, &mix).expect("cell");
        run(
            &state,
            &format!(r#"{{"method":"toggle_route","params":{{"input":"{input}","mix":"{mix}"}}}}"#),
        )
        .expect("toggle");
        let after = cell(&state, &input, &mix).expect("cell");
        assert_eq!(after.enabled, !before.enabled);
        assert_eq!(after.muted, before.muted);
        assert_eq!(after.send_percent, before.send_percent);

        run(
            &state,
            &format!(
                r#"{{"method":"adjust_route_send","params":{{"input":"{input}","mix":"{mix}","delta":-30}}}}"#
            ),
        )
        .expect("send");
        let sent = cell(&state, &input, &mix).expect("cell");
        assert_eq!(sent.send_percent, before.send_percent.saturating_sub(30));
        assert_eq!(sent.enabled, after.enabled);
    }

    #[test]
    fn unknown_targets_are_errors() {
        let _cfg = TempConfig::new("control-unknown");
        let state = app(Arc::new(MockBackend::default()));
        assert!(run(
            &state,
            r#"{"method":"adjust_input_volume","params":{"input":"nope","delta":1}}"#
        )
        .is_err());
        assert!(run(
            &state,
            r#"{"method":"set_mix_mute","params":{"mix":"nope"}}"#
        )
        .is_err());
        assert!(run(
            &state,
            r#"{"method":"toggle_route","params":{"input":"nope","mix":"nope"}}"#
        )
        .is_err());
    }

    #[test]
    fn levels_resolve_meter_keys_like_the_routing_table() {
        let _cfg = TempConfig::new("control-levels");
        let state = app(Arc::new(MockBackend::default()));
        let (input, mix) = ids(&state);
        let keys = meter_keys(&state.lock_mixer().expect("mixer").routing);
        let raw = std::collections::HashMap::from([
            (input.clone(), [0.2, 0.5]),
            (mix.clone(), [0.7, 0.1]),
        ]);
        let levels = Levels::from_raw(&keys, &raw);
        assert_eq!(levels.inputs[&input], 0.5);
        assert_eq!(levels.mixes[&mix], 0.7);
        // Silent meters are reported as zero, not left out.
        assert!(levels.inputs.values().filter(|v| **v == 0.0).count() >= 1);
    }
}
