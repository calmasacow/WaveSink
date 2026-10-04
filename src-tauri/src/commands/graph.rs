//! Bring PipeWire in line with the routing model. Every path that applies
//! routing state (startup, a profile load, a cell change, a mix rebuild) goes
//! through here, so a mix's members, send levels, level and outputs can never
//! be pushed from two different sources again.

use crate::error::SinkError;
use crate::persistence::eq::ChannelEq;
use crate::persistence::prefs::Prefs;
use crate::routing_model::{InputKind, MixDef, RoutingModel};
use crate::state::AppState;

/// Push one mix's carried inputs and each one's send level (0 = muted cell).
pub(crate) fn apply_mix_routes(
    state: &AppState,
    model: &RoutingModel,
    mix: &str,
) -> Result<(), SinkError> {
    let members = model.members(mix);
    state.backend.set_bus_members(mix, &members)?;
    for member in &members {
        state
            .backend
            .set_bus_member_gain(mix, member, model.member_gain(member, mix))?;
    }
    Ok(())
}

/// A mix node's own level and mute: what recorders and its outputs hear.
pub(crate) fn apply_mix_level(state: &AppState, mix: &MixDef) {
    if let Err(e) = state.backend.set_sink_volume(&mix.id, mix.volume_percent) {
        eprintln!("wavesink: level for mix {} failed: {e}", mix.id);
    }
    if let Err(e) = state.backend.set_sink_mute(&mix.id, mix.muted) {
        eprintln!("wavesink: mute for mix {} failed: {e}", mix.id);
    }
}

/// Create (or adopt) and fully configure one mix: node, members, sends,
/// level and outputs.
pub(crate) fn bring_up_mix(state: &AppState, model: &RoutingModel, mix: &MixDef, prefs: &Prefs) {
    if let Err(e) = state
        .backend
        .create_bus(&mix.id, &prefs.decorate(&mix.label))
    {
        eprintln!("wavesink: creating mix {} failed: {e}", mix.id);
        return;
    }
    if let Err(e) = apply_mix_routes(state, model, &mix.id) {
        eprintln!("wavesink: routes for mix {} failed: {e}", mix.id);
    }
    apply_mix_level(state, mix);
    if let Err(e) = state.backend.set_mix_outputs(&mix.id, &mix.output_bindings) {
        eprintln!("wavesink: outputs for mix {} failed: {e}", mix.id);
    }
}

/// Make the whole graph match `model`: channels at their levels with their
/// EQ, hardware inputs with their Audio FX, then every mix. Creating is
/// idempotent (existing nodes are adopted), so this serves startup and a
/// profile load alike; removing what `model` no longer has is the caller's.
pub(crate) fn bring_up(
    state: &AppState,
    model: &RoutingModel,
    eq: &ChannelEq,
    prefs: &Prefs,
) -> Result<(), String> {
    for channel in model.channels() {
        state
            .backend
            .create_virtual_sink(&channel.id, &prefs.decorate(&channel.label))
            .map_err(|e| e.to_string())?;
        // An adopted sink from a previous run may carry a stale level.
        state
            .backend
            .set_sink_volume(&channel.id, channel.volume_percent)
            .map_err(|e| e.to_string())?;
        state
            .backend
            .set_sink_mute(&channel.id, channel.muted)
            .map_err(|e| e.to_string())?;
        if let Some(config) = eq.configs.get(&channel.id) {
            if let Err(e) = state.backend.set_channel_eq(&channel.id, config) {
                eprintln!("wavesink: eq restore for {} failed: {e}", channel.id);
            }
        }
    }
    // Hardware inputs before the mixes: a mix's send levels for them are only
    // accepted once the backend knows the input.
    for input in model
        .inputs
        .iter()
        .filter(|i| i.kind == InputKind::Hardware)
    {
        if let Err(e) = state.backend.set_hardware_input(
            &input.id,
            &input.source_name,
            input.volume_percent,
            input.muted,
        ) {
            eprintln!("wavesink: hardware input {} failed: {e}", input.id);
        }
        if let Err(e) = state.backend.set_input_fx(&input.id, &input.fx) {
            eprintln!("wavesink: audio fx for {} failed: {e}", input.id);
        }
    }
    for mix in &model.mixes {
        bring_up_mix(state, model, mix, prefs);
    }
    Ok(())
}
