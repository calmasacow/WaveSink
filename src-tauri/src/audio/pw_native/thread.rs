//! The PipeWire main-loop thread. All PipeWire objects live here (they are
//! not Send); the `PipeWireBackend` facade talks to this thread through a
//! pipewire channel, and each command carries an mpsc reply sender.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;

use pipewire as pw;
use pw::core::CoreRc;
use pw::metadata::{Metadata, MetadataListener};
use pw::node::{Node, NodeListener};
use pw::registry::{GlobalObject, RegistryRc};
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

use crate::audio::pw_native::eq_chain::EqChainHandle;
use crate::audio::pw_native::input_fx::InputFxHandle;
use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::meter::MeterHandle;
use crate::audio::pw_native::pods;
use crate::audio::pw_native::send_gain::SendGainHandle;
use crate::audio::types::{is_own_sink, is_virtual_sink, AppStream, EqConfig, OutputDevice};
use crate::error::SinkError;
use crate::persistence::buses::is_bus_name;

const STREAM_CLASS: &str = "Stream/Output/Audio";
const SINK_CLASS: &str = "Audio/Sink";
const SOURCE_CLASS: &str = "Audio/Source";
const VIRTUAL_SOURCE_CLASS: &str = "Audio/Source/Virtual";
/// node.name prefix of all our internal helper streams (meters, mic chain) -
/// excluded from stream listings and node tracking.
pub const INTERNAL_PREFIX: &str = "sink-internal-";
/// node.name prefix of our meter capture streams.
pub const METER_PREFIX: &str = "sink-internal-meter-";

type Reply<T> = mpsc::Sender<Result<T, SinkError>>;
/// A set of live links: (output port, input port, proxy).
type LinkSet = Vec<(u32, u32, pw::link::Link)>;

pub enum Cmd {
    CreateSink {
        name: String,
        label: String,
        reply: Reply<()>,
    },
    DestroySink {
        name: String,
        reply: Reply<()>,
    },
    ListStreams {
        reply: Reply<Vec<AppStream>>,
    },
    ListOutputs {
        reply: Reply<Vec<OutputDevice>>,
    },
    SetNodeVolumeByName {
        name: String,
        percent: u8,
        reply: Reply<()>,
    },
    /// Pause or resume every meter stream (window hidden / shown).
    SetMetersActive {
        active: bool,
        reply: Reply<()>,
    },
    SetNodeMuteByName {
        name: String,
        muted: bool,
        reply: Reply<()>,
    },
    SetNodeVolumeById {
        id: u32,
        percent: u8,
        reply: Reply<()>,
    },
    MoveStream {
        id: u32,
        sink_name: String,
        reply: Reply<()>,
    },
    /// Create a mix bus (capturable virtual source).
    CreateBus {
        name: String,
        label: String,
        reply: Reply<()>,
    },
    /// Destroy a mix bus and its links.
    DestroyBus {
        name: String,
        reply: Reply<()>,
    },
    /// Replace the channel set feeding a bus.
    SetBusMembers {
        name: String,
        channels: Vec<String>,
        reply: Reply<()>,
    },
    /// Set one member's send level within one specific mix (0-100%).
    SetBusMemberGain {
        bus_name: String,
        member: String,
        percent: u8,
        reply: Reply<()>,
    },
    SetHardwareInput {
        id: String,
        source_name: String,
        volume_percent: u8,
        muted: bool,
        reply: Reply<()>,
    },
    RemoveHardwareInput {
        id: String,
        reply: Reply<()>,
    },
    SetMixOutputs {
        name: String,
        outputs: Vec<crate::routing_model::OutputBinding>,
        reply: Reply<()>,
    },
    /// A hardware input's Audio FX (build/drop/re-tune its chain).
    SetInputFx {
        id: String,
        fx: crate::routing_model::FxChain,
        reply: Reply<()>,
    },
    /// Apply a channel's parametric EQ (create/destroy/re-tune the insert).
    SetChannelEq {
        sink_name: String,
        config: EqConfig,
        reply: Reply<()>,
    },
    /// Hardware capture devices (microphones).
    ListInputs {
        reply: Reply<Vec<OutputDevice>>,
    },
}

struct PortEntry {
    id: u32,
    node_id: u32,
    /// "in" (playback/sink input port) or "out" (source/monitor port).
    direction: String,
    /// e.g. "FL", "FR", "MONO".
    channel: Option<String>,
    /// Position within its node and direction (`port.id`). Global ids follow
    /// registration order, which can differ from channel order.
    index: Option<u32>,
}

struct NodeEntry {
    id: u32,
    serial: Option<u64>,
    media_class: String,
    props: HashMap<String, String>,
    proxy: Node,
    _listener: NodeListener,
    volume_percent: u8,
    channels: usize,
    muted: bool,
    /// True while the node is in the Running state (actively streaming).
    active: bool,
}

/// Nodes through pipewire-pulse carry few props; pid and sandbox facts live
/// here.
struct ClientEntry {
    props: HashMap<String, String>,
    /// Set once the info event delivered the full property dict.
    settled: bool,
    _proxy: pw::client::Client,
    _listener: pw::client::ClientListener,
}

#[derive(Default, Clone, Copy)]
struct NodeLevel {
    volume_percent: Option<u8>,
    muted: Option<bool>,
}

#[derive(Default)]
struct State {
    nodes: HashMap<u32, NodeEntry>,
    clients: HashMap<u32, ClientEntry>,
    /// link global id -> (output node id, input node id)
    links: HashMap<u32, (u32, u32)>,
    metadata: Option<Metadata>,
    _metadata_listener: Option<MetadataListener>,
    default_sink_name: Option<String>,
    default_source_name: Option<String>,
    /// Virtual sinks we created: name -> created-object proxy (kept alive;
    /// destroyed explicitly on teardown).
    owned_sinks: HashMap<String, Node>,
    /// Sinks that existed before us (e.g. left behind by an earlier run): name ->
    /// global id.
    adopted_sinks: HashMap<String, u32>,
    /// Nodes that must stay alive; if one vanishes without us destroying it
    /// (another instance, a PipeWire restart, wpctl), it is recreated in place.
    desired: HashMap<String, (String, NodeKind)>,
    /// Last level and mute set on each `desired` node, put back when the node
    /// is recreated (a fresh node starts at 100% and unmuted).
    node_levels: HashMap<String, NodeLevel>,
    /// Create requests waiting for the sink's global to appear.
    pending_creates: HashMap<String, Vec<Reply<()>>>,
    /// Live meter capture streams per node name: our channels and mixes,
    /// plus every hardware input and output device.
    meters: HashMap<String, MeterHandle>,
    /// Meters are paused while the window is hidden; new ones start paused.
    meters_paused: bool,
    /// All known ports, for monitor→output linking.
    ports: HashMap<u32, PortEntry>,
    /// Hardware input id -> its Audio FX settings.
    input_fx_configs: HashMap<String, crate::routing_model::FxChain>,
    /// Hardware input id -> live FX chain, only while a stage is on and the
    /// device is present.
    input_fx: HashMap<String, InputFxHandle>,
    levels: Option<Arc<LevelStore>>,
    /// Mix buses we own: node name -> proxy.
    bus_sources: HashMap<String, Node>,
    /// Bus node name -> member channel sink names.
    bus_members: HashMap<String, std::collections::HashSet<String>>,
    /// (bus, member) -> live links feeding the bus (a gained pair carries
    /// the insert's playback→bus leg instead).
    bus_links: HashMap<(String, String), LinkSet>,
    /// (bus, member) -> send level (0-100%). Absent = 100%, direct link.
    bus_member_gains: HashMap<(String, String), u8>,
    /// Unscaled hardware route faders. `bus_member_gains` holds live values
    /// after source fader/mute scaling, so this survives source adjustments.
    hardware_route_gains: HashMap<(String, String), u8>,
    /// Matrix hardware input id -> (PipeWire source node.name, local level, muted).
    hardware_inputs: HashMap<String, (String, u8, bool)>,
    /// (bus, member) -> live gain insert, only while that pair is off unity.
    send_gains: HashMap<(String, String), SendGainHandle>,
    /// (bus, member) -> links from the member's source into its insert.
    send_gain_in_links: HashMap<(String, String), LinkSet>,
    /// Pairs whose insert failed to build - not retried until the user
    /// touches that gain, so a persistent failure can't log every event.
    send_gain_failed: std::collections::HashSet<(String, String)>,
    /// Mix name -> persisted enabled physical output node names.
    mix_outputs: HashMap<String, Vec<String>>,
    /// (mix, output node name) -> live links.
    mix_output_links: HashMap<(String, String), LinkSet>,
    /// Per-channel EQ configs (source of truth for chain (re)creation -
    /// kept even while disabled so re-enabling restores the bands).
    eq_configs: HashMap<String, EqConfig>,
    /// Live EQ inserts by channel sink name. Presence *is* "EQ enabled and
    /// live" - the channel's outgoing links then re-source from the insert.
    eq_streams: HashMap<String, EqChainHandle>,
    /// EQ playback node id -> node ids it may feed. The link police destroys
    /// anything else, since WirePlumber routes playback to the default sink.
    eq_desired_targets: HashMap<u32, std::collections::HashSet<u32>>,
}

impl State {
    /// Live node id of a hardware input's FX playback stream, if it's up.
    fn fx_playback_node(&self, input_id: &str) -> Option<u32> {
        self.input_fx
            .get(input_id)
            .map(|h| h.playback_node_id())
            .filter(|id| *id != u32::MAX)
    }

    /// Live node id of a channel's EQ playback stream, if the insert is up.
    fn eq_playback_node(&self, sink_name: &str) -> Option<u32> {
        self.eq_streams
            .get(sink_name)
            .map(|h| h.playback_node_id())
            .filter(|id| *id != u32::MAX)
    }
}

/// Node whose ports feed a channel's downstream links: the EQ insert's
/// playback stream when live, else the channel sink. Pure for testability.
fn resolve_source(eq_playback: Option<u32>, channel_id: u32) -> u32 {
    eq_playback.unwrap_or(channel_id)
}

impl State {
    fn node_by_name(&self, name: &str) -> Option<&NodeEntry> {
        self.nodes
            .values()
            .find(|n| n.props.get("node.name").map(String::as_str) == Some(name))
    }

    /// The sink a stream is currently connected to, resolved through links.
    fn sink_of_stream(&self, stream_id: u32) -> Option<&NodeEntry> {
        self.links
            .values()
            .find(|(out, _)| *out == stream_id)
            .and_then(|(_, input)| self.nodes.get(input))
    }
}

// Needed by the command handler for object creation/destruction; a
// thread-local is the simplest way to share it across listener closures.
thread_local! {
    static CORE: RefCell<Option<CoreRc>> = const { RefCell::new(None) };
}

/// Entry point: runs the PipeWire loop until the channel closes.
/// `init_tx` reports startup success/failure exactly once.
pub fn run(
    receiver: pw::channel::Receiver<Cmd>,
    init_tx: mpsc::Sender<Result<(), SinkError>>,
    levels: Arc<LevelStore>,
) {
    if let Err(e) = setup_and_run(receiver, &init_tx, levels) {
        let _ = init_tx.send(Err(e));
    }
}

fn setup_and_run(
    receiver: pw::channel::Receiver<Cmd>,
    init_tx: &mpsc::Sender<Result<(), SinkError>>,
    levels: Arc<LevelStore>,
) -> Result<(), SinkError> {
    pw::init();
    let err = |stage: &str, e: pw::Error| SinkError::Config(format!("pipewire {stage}: {e}"));

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| err("mainloop", e))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| err("context", e))?;
    let core = context.connect_rc(None).map_err(|e| err("connect", e))?;
    let registry = core.get_registry_rc().map_err(|e| err("registry", e))?;

    CORE.with(|c| *c.borrow_mut() = Some(core.clone()));

    let state = Rc::new(RefCell::new(State {
        levels: Some(levels.clone()),
        ..State::default()
    }));

    // ---- registry listeners ----
    let state_g = state.clone();
    let registry_g = registry.clone();
    let core_g = core.clone();
    let levels_g = levels.clone();
    let _reg_listener = registry
        .add_listener_local()
        .global(move |global| {
            on_global(&state_g, &registry_g, &core_g, &levels_g, global);
        })
        .global_remove({
            let state = state.clone();
            move |id| {
                enum Heal {
                    Nothing,
                    Relink,
                    Recreate(String, String, NodeKind),
                }
                let heal = {
                    let mut s = state.borrow_mut();
                    s.links.remove(&id);
                    s.ports.remove(&id);
                    s.clients.remove(&id);
                    let Some(node) = s.nodes.remove(&id) else {
                        return;
                    };
                    let name = node.props.get("node.name").cloned().unwrap_or_default();
                    if node.media_class == SINK_CLASS {
                        s.adopted_sinks.remove(&name);
                    }
                    // Our nodes keep their slot for the recreate; an unplugged
                    // device frees its own.
                    if s.meters.remove(&name).is_some() && !is_own_sink(&name) {
                        if let Some(levels) = &s.levels {
                            levels.release(&name);
                        }
                    }
                    // An FX chain's device left. Drop the chain so it
                    // rebuilds when the device returns.
                    if node.media_class == SOURCE_CLASS {
                        s.input_fx.retain(|_, fx| fx.source_name != name);
                    }
                    match s.desired.get(&name).cloned() {
                        Some((label, kind)) => {
                            // Drop any dangling proxy so the heal isn't
                            // blocked by a corpse.
                            match kind {
                                NodeKind::Channel => {
                                    s.owned_sinks.remove(&name);
                                }
                                NodeKind::MixSource => {
                                    s.bus_sources.remove(&name);
                                }
                            }
                            // Proxy is replaced before this event fires, so
                            // `is_some()` can't tell recreate from destroy.
                            let already_back = s.node_by_name(&name).is_some();
                            if already_back {
                                Heal::Relink
                            } else {
                                s.meters.remove(&name);
                                // The insert never reconnects, so it must be
                                // dropped for the rebuild hook in `on_node`.
                                s.eq_streams.remove(&name);
                                Heal::Recreate(name, label, kind)
                            }
                        }
                        // An output device vanished: relink so affected
                        // channels fail over to the default.
                        None if node.media_class == SINK_CLASS => Heal::Relink,
                        None => Heal::Nothing,
                    }
                };
                match heal {
                    Heal::Recreate(name, label, kind) => {
                        eprintln!("wavesink: {name} vanished externally - recreating");
                        if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                            match create_node_object(&core, &name, &label, kind) {
                                Ok(proxy) => {
                                    let mut s = state.borrow_mut();
                                    match kind {
                                        NodeKind::Channel => {
                                            s.owned_sinks.insert(name, proxy);
                                        }
                                        NodeKind::MixSource => {
                                            s.bus_sources.insert(name, proxy);
                                        }
                                    }
                                }
                                Err(e) => eprintln!("wavesink: recreate {name} failed: {e}"),
                            }
                        }
                        ensure_all_links(&state);
                    }
                    Heal::Relink => ensure_all_links(&state),
                    Heal::Nothing => {}
                }
            }
        })
        .register();

    // ---- command channel ----
    let state_c = state.clone();
    let registry_c = registry.clone();
    let _recv = receiver.attach(mainloop.loop_(), move |cmd| {
        handle_cmd(&state_c, &registry_c, cmd);
    });

    init_tx
        .send(Ok(()))
        .map_err(|_| SinkError::Config("backend owner vanished during init".into()))?;

    mainloop.run();
    Ok(())
}

fn on_global(
    state: &Rc<RefCell<State>>,
    registry: &RegistryRc,
    core: &CoreRc,
    levels: &Arc<LevelStore>,
    global: &GlobalObject<&DictRef>,
) {
    match global.type_ {
        ObjectType::Node => on_node(state, registry, core, levels, global),
        ObjectType::Client => on_client(state, registry, global),
        ObjectType::Port => {
            let Some(props) = global.props else { return };
            let Some(node_id) = props.get("node.id").and_then(|v| v.parse().ok()) else {
                return;
            };
            let entry = PortEntry {
                id: global.id,
                node_id,
                direction: props.get("port.direction").unwrap_or_default().to_string(),
                channel: props.get("audio.channel").map(str::to_string),
                index: props.get("port.id").and_then(|v| v.parse().ok()),
            };
            state.borrow_mut().ports.insert(global.id, entry);
            // Wiring depends on ports of untracked stream nodes (EQ, FX and
            // gain inserts), so reconcile every port event; no-op until ready.
            ensure_all_links(state);
        }
        ObjectType::Link => {
            let Some(props) = global.props else { return };
            let out = props.get("link.output.node").and_then(|v| v.parse().ok());
            let inp = props.get("link.input.node").and_then(|v| v.parse().ok());
            if let (Some(out), Some(inp)) = (out, inp) {
                let police = {
                    let mut s = state.borrow_mut();
                    s.links.insert(global.id, (out, inp));
                    // Same policing for EQ playback: only planned links may
                    // exist - a node with no plan yet allows nothing.
                    let eq_stray = s.eq_streams.values().any(|h| h.playback_node_id() == out)
                        && !s
                            .eq_desired_targets
                            .get(&out)
                            .is_some_and(|allowed| allowed.contains(&inp));
                    // Send-gain inserts are policed on both ends too: an
                    // unplanned link could leak gained audio to output.
                    let allowed = |out: u32, inp: u32| {
                        s.eq_desired_targets
                            .get(&out)
                            .is_some_and(|allowed| allowed.contains(&inp))
                    };
                    let send_stray = (s.send_gains.values().any(|h| h.playback_node_id() == out)
                        || s.send_gains.values().any(|h| h.capture_node_id() == inp))
                        && !allowed(out, inp);
                    // FX playback feeds only its planned mixes.
                    let fx_stray = s.input_fx.values().any(|h| h.playback_node_id() == out)
                        && !allowed(out, inp);
                    eq_stray || send_stray || fx_stray
                };
                if police {
                    let _ = registry.destroy_global(global.id);
                }
            }
        }
        ObjectType::Metadata => {
            let Some(props) = global.props else { return };
            if props.get("metadata.name") != Some("default") {
                return;
            }
            let Ok(metadata) = registry.bind::<Metadata, _>(global) else {
                return;
            };
            let state_m = state.clone();
            let listener = metadata
                .add_listener_local()
                .property(move |_subject, key, _type, value| {
                    // values are JSON like {"name":"alsa_output...."}
                    let parse_name = |v: Option<&str>| {
                        v.and_then(|v| {
                            serde_json::from_str::<serde_json::Value>(v)
                                .ok()?
                                .get("name")?
                                .as_str()
                                .map(str::to_string)
                        })
                    };
                    if key == Some("default.audio.sink") {
                        let name = parse_name(value);
                        let changed = {
                            let mut s = state_m.borrow_mut();
                            let changed = s.default_sink_name != name;
                            s.default_sink_name = name;
                            changed
                        };
                        // Channels following the default must relink
                        // (Sonar-style automatic device failover).
                        if changed {
                            ensure_all_links(&state_m);
                        }
                    } else if key == Some("default.audio.source") {
                        let name = parse_name(value);
                        state_m.borrow_mut().default_source_name = name;
                    }
                    0
                })
                .register();
            let mut s = state.borrow_mut();
            s.metadata = Some(metadata);
            s._metadata_listener = Some(listener);
        }
        _ => {}
    }
}

fn on_client(state: &Rc<RefCell<State>>, registry: &RegistryRc, global: &GlobalObject<&DictRef>) {
    let Ok(proxy) = registry.bind::<pw::client::Client, _>(global) else {
        return;
    };
    let props: HashMap<String, String> = global
        .props
        .map(|d| {
            d.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default();
    // The full dict (sec.pid, portal app id) only arrives with the info event.
    let state_i = state.clone();
    let client_id = global.id;
    let listener = proxy
        .add_listener_local()
        .info(move |info| {
            let mut s = state_i.borrow_mut();
            if let (Some(entry), Some(props)) = (s.clients.get_mut(&client_id), info.props()) {
                for (k, v) in props.iter() {
                    entry.props.insert(k.to_string(), v.to_string());
                }
                entry.settled = true;
            }
        })
        .register();
    state.borrow_mut().clients.insert(
        global.id,
        ClientEntry {
            props,
            settled: false,
            _proxy: proxy,
            _listener: listener,
        },
    );
}

fn on_node(
    state: &Rc<RefCell<State>>,
    registry: &RegistryRc,
    core: &CoreRc,
    levels: &Arc<LevelStore>,
    global: &GlobalObject<&DictRef>,
) {
    let Some(dict) = global.props else { return };
    let media_class = dict.get("media.class").unwrap_or_default().to_string();
    if media_class != STREAM_CLASS
        && media_class != SINK_CLASS
        && media_class != SOURCE_CLASS
        && media_class != VIRTUAL_SOURCE_CLASS
    {
        return;
    }
    let props: HashMap<String, String> = dict
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let node_name = props.get("node.name").cloned().unwrap_or_default();
    // Never track our own internal helper streams (meters, mic chain).
    if node_name.starts_with(INTERNAL_PREFIX) {
        return;
    }

    let Ok(proxy) = registry.bind::<Node, _>(global) else {
        return;
    };

    // Track volume/mute through Props param events, and the running state
    // through info events (drives the app-list activity indicator).
    let state_p = state.clone();
    let state_i = state.clone();
    let node_id = global.id;
    let listener = proxy
        .add_listener_local()
        .info(move |info| {
            let running = matches!(info.state(), pw::node::NodeState::Running);
            let mut s = state_i.borrow_mut();
            if let Some(entry) = s.nodes.get_mut(&node_id) {
                entry.active = running;
                // Registry globals carry only an abbreviated prop set; the info
                // event has the full dict.
                if let Some(props) = info.props() {
                    for (k, v) in props.iter() {
                        entry.props.insert(k.to_string(), v.to_string());
                    }
                }
            }
        })
        .param(move |_seq, id, _index, _next, param| {
            if id != pw::spa::param::ParamType::Props {
                return;
            }
            let Some(pod) = param else { return };
            let parsed = pods::parse_props(pod);
            let mut s = state_p.borrow_mut();
            if let Some(entry) = s.nodes.get_mut(&node_id) {
                if let Some(linear) = parsed.volume_linear {
                    entry.volume_percent = pods::linear_to_percent(linear);
                }
                if let Some(channels) = parsed.channels {
                    entry.channels = channels;
                }
                if let Some(muted) = parsed.muted {
                    entry.muted = muted;
                }
            }
        })
        .register();
    proxy.subscribe_params(&[pw::spa::param::ParamType::Props]);

    let entry = NodeEntry {
        id: global.id,
        serial: props.get("object.serial").and_then(|v| v.parse().ok()),
        media_class: media_class.clone(),
        props,
        proxy,
        _listener: listener,
        volume_percent: 100,
        channels: 2,
        muted: false,
        active: false,
    };

    let mut s = state.borrow_mut();
    s.nodes.insert(global.id, entry);

    // Hardware inputs: meter what the device hears.
    if media_class == SOURCE_CLASS && !s.meters.contains_key(&node_name) {
        add_meter(&mut s, core, &node_name, global.id, levels, false);
    }

    if media_class == SINK_CLASS && is_virtual_sink(&node_name) {
        if let Some(waiters) = s.pending_creates.remove(&node_name) {
            for reply in waiters {
                let _ = reply.send(Ok(()));
            }
        }
        if !s.owned_sinks.contains_key(&node_name) {
            s.adopted_sinks.insert(node_name.clone(), global.id);
        }
        restore_level(&s, &node_name, global.id);
        if !s.meters.contains_key(&node_name) {
            add_meter(&mut s, core, &node_name, global.id, levels, true);
        }
        // An enabled EQ config with no live insert: build it against the fresh
        // sink id. Covers both startup and the heal path with the same hook.
        if !s.eq_streams.contains_key(&node_name) {
            if let Some(config) = s.eq_configs.get(&node_name).filter(|c| c.enabled).cloned() {
                match EqChainHandle::new(core, &node_name, global.id, &config) {
                    Ok(handle) => {
                        s.eq_streams.insert(node_name.clone(), handle);
                    }
                    Err(e) => eprintln!("wavesink: eq chain for {node_name} failed: {e}"),
                }
            }
        }
        drop(s);
        ensure_all_links(state);
        return;
    }

    // A mix in the playback list has no source of its own to meter, so it
    // is metered through its monitor, like a channel.
    if (media_class == VIRTUAL_SOURCE_CLASS || media_class == SINK_CLASS) && is_bus_name(&node_name)
    {
        if let Some(waiters) = s.pending_creates.remove(&node_name) {
            for reply in waiters {
                let _ = reply.send(Ok(()));
            }
        }
        restore_level(&s, &node_name, global.id);
        let from_monitor = media_class == SINK_CLASS;
        if !s.meters.contains_key(&node_name) {
            add_meter(&mut s, core, &node_name, global.id, levels, from_monitor);
        }
        drop(s);
        ensure_all_links(state);
        return;
    }

    // A hardware input's device: plugged in now, or back after a restart.
    // Its FX chain (if any) and route links come up through the reconcile.
    if media_class == SOURCE_CLASS {
        drop(s);
        ensure_all_links(state);
        return;
    }

    drop(s);
    // A new hardware sink may be the (returning) target of a channel.
    if media_class == SINK_CLASS {
        ensure_all_links(state);
    }
}

/// Both classes: a noise suppressor publishes its cleaned-up mic as a
/// virtual source, not a real device, and the chain can capture either.
/// Start a meter on `name`, paused if the window is hidden.
fn add_meter(
    s: &mut State,
    core: &CoreRc,
    name: &str,
    id: u32,
    levels: &Arc<LevelStore>,
    capture_sink: bool,
) {
    match MeterHandle::new(core, name, id, levels.clone(), capture_sink) {
        Ok(meter) => {
            if s.meters_paused {
                meter.set_active(false);
            }
            s.meters.insert(name.to_string(), meter);
        }
        Err(e) => eprintln!("wavesink: meter for {name} failed: {e}"),
    }
}

/// Compute monitor→input port pairs from `channel_id`'s output ports to
/// `target_id`'s input ports, by audio.channel with an index-wrap fallback.
fn desired_pairs(s: &State, channel_id: u32, target_id: u32) -> Vec<(u32, u32)> {
    if channel_id == target_id {
        return Vec::new();
    }
    let mut monitors: Vec<&PortEntry> = s
        .ports
        .values()
        .filter(|p| p.node_id == channel_id && p.direction == "out")
        .collect();
    let mut inputs: Vec<&PortEntry> = s
        .ports
        .values()
        .filter(|p| p.node_id == target_id && p.direction == "in")
        .collect();
    // Node-local order, so an unnamed-channel fallback (pro-audio AUX0/AUX1
    // ports) maps FL to the first port and FR to the second for every node.
    monitors.sort_by_key(|p| (p.index.unwrap_or(u32::MAX), p.id));
    inputs.sort_by_key(|p| (p.index.unwrap_or(u32::MAX), p.id));
    if monitors.is_empty() || inputs.is_empty() {
        return Vec::new();
    }
    // Mono source into a multi-channel target: fan out to every input
    // (e.g. listening to the mic - both ears, not just FL).
    if monitors.len() == 1 && inputs.len() > 1 {
        let m = monitors[0];
        return inputs.iter().map(|p| (m.id, p.id)).collect();
    }
    monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let by_channel = m.channel.as_ref().and_then(|ch| {
                inputs
                    .iter()
                    .find(|p| p.channel.as_ref() == Some(ch))
                    .copied()
            });
            let input = by_channel.or_else(|| inputs.get(i % inputs.len()).copied())?;
            Some((m.id, input.id))
        })
        .collect()
}

/// Highest-`priority.session` non-virtual sink from the candidates - reuses
/// WirePlumber's own scoring so the fallback matches the OS's pick.
fn pick_fallback_sink<'a>(candidates: impl Iterator<Item = (u32, &'a str, i64)>) -> Option<u32> {
    candidates
        .filter(|(_, name, _)| !is_own_sink(name))
        .max_by_key(|&(_, _, priority)| priority)
        .map(|(id, _, _)| id)
}

/// The real output sink to fall back to when a follow-default channel's default
/// has no live node (e.g. device unplugged, WirePlumber hasn't reassigned yet).
fn fallback_sink(s: &State) -> Option<u32> {
    pick_fallback_sink(
        s.nodes
            .values()
            .filter(|n| n.media_class == SINK_CLASS)
            .map(|n| {
                (
                    n.id,
                    n.props.get("node.name").map(String::as_str).unwrap_or(""),
                    n.props
                        .get("priority.session")
                        .and_then(|p| p.parse::<i64>().ok())
                        .unwrap_or(0),
                )
            }),
    )
}

/// Create link objects for `pairs` between two nodes; returns the proxies.
fn create_links(
    core: &CoreRc,
    sink_name: &str,
    out_node: u32,
    in_node: u32,
    pairs: &[(u32, u32)],
) -> LinkSet {
    let mut created = Vec::new();
    for (monitor_port, input_port) in pairs {
        match core.create_object::<pw::link::Link>(
            "link-factory",
            &pw::properties::properties! {
                "link.output.node" => out_node.to_string(),
                "link.output.port" => monitor_port.to_string(),
                "link.input.node" => in_node.to_string(),
                "link.input.port" => input_port.to_string(),
            },
        ) {
            Ok(link) => created.push((*monitor_port, *input_port, link)),
            Err(e) => eprintln!("wavesink: link {sink_name} failed: {e}"),
        }
    }
    created
}

/// One member's candidate contribution to one bus.
struct MemberLink<'a> {
    bus_name: &'a str,
    bus_id: u32,
    /// A channel sink name, or a hardware input id.
    member: &'a str,
    /// A channel's EQ playback when live, else the channel/mic node itself.
    source_id: u32,
    included: bool,
}

/// Route one member into one bus: a direct link at unity, or a gain insert when
/// off 100%. Every planned leg is registered in `eq_targets` for policing.
fn reconcile_bus_member(
    core: &CoreRc,
    s: &mut State,
    link: MemberLink,
    eq_targets: &mut HashMap<u32, std::collections::HashSet<u32>>,
) {
    let MemberLink {
        bus_name,
        bus_id,
        member,
        source_id,
        included,
    } = link;
    let key = (bus_name.to_string(), member.to_string());
    let gain = s.bus_member_gains.get(&key).copied().unwrap_or(100);

    if !included || gain == 100 {
        // Tear down any insert; the failure marker goes with it so a
        // re-included member retries the build.
        s.send_gain_failed.remove(&key);
        if s.send_gains.remove(&key).is_some() {
            s.send_gain_in_links.remove(&key);
            s.bus_links.remove(&key);
        }
        let pairs = if included {
            desired_pairs(&*s, source_id, bus_id)
        } else {
            Vec::new()
        };
        let current: Vec<(u32, u32)> = s
            .bus_links
            .get(&key)
            .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
            .unwrap_or_default();
        if current != pairs {
            s.bus_links.remove(&key);
            if !pairs.is_empty() {
                let created = create_links(core, member, source_id, bus_id, &pairs);
                if !created.is_empty() {
                    s.bus_links.insert(key.clone(), created);
                }
            }
        }
        if included {
            eq_targets.entry(source_id).or_default().insert(bus_id);
        }
        return;
    }

    // Off-unity: route through a gain insert instead of a direct link.
    if !s.send_gains.contains_key(&key) {
        if s.send_gain_failed.contains(&key) {
            return;
        }
        // The insert takes over from any leftover direct link.
        s.bus_links.remove(&key);
        let insert_key = format!("{bus_name}-{member}");
        match SendGainHandle::new(core, &insert_key, gain) {
            Ok(handle) => {
                s.send_gains.insert(key.clone(), handle);
            }
            Err(e) => {
                eprintln!("wavesink: send gain for {member} in {bus_name} failed: {e}");
                s.send_gain_failed.insert(key);
                return;
            }
        }
    }
    let Some(handle) = s.send_gains.get(&key) else {
        return;
    };
    let (capture_id, playback_id) = (handle.capture_node_id(), handle.playback_node_id());
    // Ids unassigned until the server registers the streams; a later
    // port/node event re-drives this.
    if capture_id == u32::MAX || playback_id == u32::MAX {
        return;
    }

    eq_targets.entry(source_id).or_default().insert(capture_id);
    // The playback leg must be planned, or the link police can't stop
    // WirePlumber from leaking this audio to the user's output.
    eq_targets.entry(playback_id).or_default().insert(bus_id);

    // ---- member source -> gain capture ----
    let in_pairs = desired_pairs(&*s, source_id, capture_id);
    let in_current: Vec<(u32, u32)> = s
        .send_gain_in_links
        .get(&key)
        .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
        .unwrap_or_default();
    if in_current != in_pairs {
        s.send_gain_in_links.remove(&key);
        if !in_pairs.is_empty() {
            let created = create_links(core, member, source_id, capture_id, &in_pairs);
            if !created.is_empty() {
                s.send_gain_in_links.insert(key.clone(), created);
            }
        }
    }

    // ---- gain playback -> bus ----
    let out_pairs = desired_pairs(&*s, playback_id, bus_id);
    let out_current: Vec<(u32, u32)> = s
        .bus_links
        .get(&key)
        .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
        .unwrap_or_default();
    if out_current != out_pairs {
        s.bus_links.remove(&key);
        if !out_pairs.is_empty() {
            let created = create_links(core, member, playback_id, bus_id, &out_pairs);
            if !created.is_empty() {
                s.bus_links.insert(key, created);
            }
        }
    }
}

/// Bring each hardware input's FX chain in line with its settings: built while
/// a stage is on and its device is present, dropped otherwise, rebuilt if the
/// input now names a different device.
fn ensure_input_fx(core: &CoreRc, s: &mut State, node_ids: &HashMap<String, u32>) {
    let wanted: HashMap<String, (String, u32, crate::routing_model::FxChain)> = s
        .input_fx_configs
        .iter()
        .filter(|(_, fx)| fx.is_active())
        .filter_map(|(id, fx)| {
            let (source_name, _, _) = s.hardware_inputs.get(id)?;
            let source_id = node_ids.get(source_name).copied()?;
            Some((id.clone(), (source_name.clone(), source_id, fx.clone())))
        })
        .collect();
    s.input_fx.retain(|id, handle| {
        wanted
            .get(id)
            .is_some_and(|(source_name, _, _)| *source_name == handle.source_name)
    });
    let Some(levels) = s.levels.clone() else {
        return;
    };
    for (id, (source_name, source_id, fx)) in wanted {
        if s.input_fx.contains_key(&id) {
            continue;
        }
        match InputFxHandle::new(core, &id, &source_name, source_id, &fx, levels.clone()) {
            Ok(handle) => {
                s.input_fx.insert(id, handle);
            }
            Err(e) => eprintln!("wavesink: audio fx for {id} failed: {e}"),
        }
    }
}

/// Reconcile loopback links for every virtual channel: monitor -> chosen output
/// device (failover to default) and monitor -> Stream Mix. Idempotent.
fn ensure_all_links(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    // One name→id snapshot per reconcile instead of a linear node scan per
    // lookup (this runs on every relevant registry event).
    let node_ids: HashMap<String, u32> = s
        .nodes
        .values()
        .filter_map(|n| n.props.get("node.name").map(|name| (name.clone(), n.id)))
        .collect();
    // Live bus nodes: (bus name, node id).
    let bus_ids: Vec<(String, u32)> = s
        .bus_members
        .keys()
        .filter_map(|bus| node_ids.get(bus).map(|id| (bus.clone(), *id)))
        .collect();

    // Live channel set: every virtual sink we created or adopted.
    let channel_names: Vec<String> = s
        .owned_sinks
        .keys()
        .chain(s.adopted_sinks.keys())
        .cloned()
        .collect();

    // What a mix bound to "System default" plays to: the default output, or
    // the best available device when that has no live node.
    let default_output = s
        .default_sink_name
        .as_ref()
        .filter(|name| !is_own_sink(name))
        .and_then(|name| node_ids.get(name))
        .copied()
        .or_else(|| fallback_sink(&s));

    // The link plan for every live EQ insert, rebuilt each pass - the link
    // police destroys anything an EQ playback node feeds that isn't in here.
    let mut eq_targets: HashMap<u32, std::collections::HashSet<u32>> = HashMap::new();

    for sink_name in &channel_names {
        let sink_name = sink_name.as_str();
        let channel_id = match node_ids.get(sink_name) {
            Some(id) => *id,
            None => continue,
        };
        // With a live EQ insert, every link re-sources from its playback node,
        // so listeners hear the same (EQ'd, equally delayed) audio.
        let source_id = resolve_source(s.eq_playback_node(sink_name), channel_id);

        // ---- mix bus links (one set per bus, membership-gated) ----
        for (bus_name, bus_id) in &bus_ids {
            let included = s
                .bus_members
                .get(bus_name)
                .is_some_and(|members| members.contains(sink_name));
            reconcile_bus_member(
                &core,
                &mut s,
                MemberLink {
                    bus_name,
                    bus_id: *bus_id,
                    member: sink_name,
                    source_id,
                    included,
                },
                &mut eq_targets,
            );
        }
    }

    // ---- saved hardware sources → mix buses ----
    ensure_input_fx(&core, &mut s, &node_ids);
    let hardware_inputs: Vec<(String, String, u8, bool)> = s
        .hardware_inputs
        .iter()
        .map(|(id, (source, level, muted))| (id.clone(), source.clone(), *level, *muted))
        .collect();
    for (member, source_name, level, muted) in hardware_inputs {
        let Some(device_id) = node_ids.get(&source_name).copied() else {
            continue;
        };
        // With Audio FX on, mixes take the processed stream - and nothing
        // until it is up, rather than a burst of unprocessed audio.
        let (source_id, ready) = if s
            .input_fx_configs
            .get(&member)
            .is_some_and(|fx| fx.is_active())
        {
            match s.fx_playback_node(&member) {
                Some(id) => (id, true),
                None => (device_id, false),
            }
        } else {
            (device_id, true)
        };
        for (bus_name, bus_id) in &bus_ids {
            let included = s
                .bus_members
                .get(bus_name)
                .is_some_and(|members| members.contains(&member));
            let key = (bus_name.clone(), member.clone());
            let route_gain = s.hardware_route_gains.get(&key).copied().unwrap_or(100);
            let effective = if included && !muted {
                (u16::from(route_gain) * u16::from(level) / 100) as u8
            } else {
                0
            };
            if effective == 100 {
                s.bus_member_gains.remove(&key);
            } else {
                s.bus_member_gains.insert(key, effective);
            }
            reconcile_bus_member(
                &core,
                &mut s,
                MemberLink {
                    bus_name,
                    bus_id: *bus_id,
                    member: &member,
                    source_id,
                    included: effective > 0 && ready,
                },
                &mut eq_targets,
            );
        }
    }

    // ---- mix → selected physical outputs ----
    // These are persistent destinations, unlike monitor links. Reconcile by
    // name so unplug/replug events rebuild the exact requested link set.
    let mix_outputs: Vec<(String, Vec<String>)> = s
        .mix_outputs
        .iter()
        .map(|(mix, outputs)| (mix.clone(), outputs.clone()))
        .collect();
    for (mix_name, outputs) in mix_outputs {
        let Some(mix_id) = bus_ids
            .iter()
            .find(|(name, _)| name == &mix_name)
            .map(|(_, id)| *id)
        else {
            continue;
        };
        // A device reached twice (bound by name and as "System default") is
        // linked once, or the mix would play double.
        let mut linked = std::collections::HashSet::new();
        for output_name in outputs {
            let target = if output_name == crate::routing_model::SYSTEM_DEFAULT_OUTPUT {
                default_output
            } else {
                node_ids.get(&output_name).copied()
            }
            .filter(|id| linked.insert(*id));
            let pairs = target
                .map(|target| desired_pairs(&s, mix_id, target))
                .unwrap_or_default();
            let key = (mix_name.clone(), output_name);
            let current: Vec<(u32, u32)> = s
                .mix_output_links
                .get(&key)
                .map(|links| links.iter().map(|(out, input, _)| (*out, *input)).collect())
                .unwrap_or_default();
            if current != pairs {
                s.mix_output_links.remove(&key);
                if let (Some(target), false) = (target, pairs.is_empty()) {
                    let created = create_links(&core, &mix_name, mix_id, target, &pairs);
                    if !created.is_empty() {
                        s.mix_output_links.insert(key, created);
                    }
                }
            }
        }
    }

    // Publish the EQ link plan for the police (see on_global's Link arm).
    s.eq_desired_targets = eq_targets;
}

/// A node WaveSink creates and keeps alive. Mixes are capture-only sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Channel,
    MixSource,
}

impl NodeKind {
    fn is_mix(self) -> bool {
        matches!(self, Self::MixSource)
    }

    fn media_class(self) -> &'static str {
        match self {
            Self::Channel => SINK_CLASS,
            Self::MixSource => VIRTUAL_SOURCE_CLASS,
        }
    }
}

fn create_node_object(
    core: &CoreRc,
    name: &str,
    label: &str,
    kind: NodeKind,
) -> Result<Node, pw::Error> {
    let props = pw::properties::properties! {
        "factory.name" => "support.null-audio-sink",
        "node.name" => name,
        "node.description" => label,
        "media.class" => kind.media_class(),
        "audio.position" => "[ FL FR ]",
        // Everything the user sets a level on needs its monitor to follow it.
        "monitor.channel-volumes" => "true",
        // WaveSink owns these levels; WirePlumber restoring an old saved
        // volume (e.g. a pre-cap 150%) would override and amplify.
        "state.restore-props" => "false",
    };
    core.create_object::<Node>("adapter", &props)
}

fn handle_cmd(state: &Rc<RefCell<State>>, registry: &RegistryRc, cmd: Cmd) {
    match cmd {
        Cmd::CreateSink { name, label, reply } => {
            let mut s = state.borrow_mut();
            if s.node_by_name(&name).is_some() {
                // Already exists (e.g. leftover from a previous run) - the
                // registry handler has adopted it. Still ours to keep alive.
                s.desired.insert(name, (label, NodeKind::Channel));
                let _ = reply.send(Ok(()));
                return;
            }
            if !is_virtual_sink(&name) {
                let _ = reply.send(Err(SinkError::UnknownSink(name)));
                return;
            }
            let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                let _ = reply.send(Err(SinkError::Config(
                    "sink creation requires a live core".into(),
                )));
                return;
            };
            match core.create_object::<Node>(
                "adapter",
                &pw::properties::properties! {
                    "factory.name" => "support.null-audio-sink",
                    "node.name" => name.as_str(),
                    "node.description" => label.as_str(),
                    "media.class" => SINK_CLASS,
                    "audio.position" => "[ FL FR ]",
                    "monitor.channel-volumes" => "true",
                    "state.restore-props" => "false",
                },
            ) {
                // The created proxy must be kept alive until teardown. The
                // reply fires when the global appears in the registry.
                Ok(proxy) => {
                    s.owned_sinks.insert(name.clone(), proxy);
                    s.desired.insert(name.clone(), (label, NodeKind::Channel));
                    s.pending_creates.entry(name).or_default().push(reply);
                }
                Err(e) => {
                    let _ = reply.send(Err(SinkError::Config(format!("create sink: {e}"))));
                }
            }
        }
        Cmd::DestroySink { name, reply } => {
            let mut s = state.borrow_mut();
            s.desired.remove(&name);
            s.node_levels.remove(&name);
            s.meters.remove(&name);
            // Drop the EQ insert before the sink proxy goes away so the
            // capture stream's target doesn't vanish under it mid-teardown.
            s.eq_streams.remove(&name);
            s.eq_configs.remove(&name);
            s.bus_links.retain(|(_, ch), _| ch != &name);
            s.send_gains.retain(|(_, ch), _| ch != &name);
            s.send_gain_in_links.retain(|(_, ch), _| ch != &name);
            s.bus_member_gains.retain(|(_, ch), _| ch != &name);
            s.send_gain_failed.retain(|(_, ch)| ch != &name);
            if let Some(levels) = &s.levels {
                levels.release(&name);
            }
            if let Some(proxy) = s.owned_sinks.remove(&name) {
                match CORE.with(|c| c.borrow().clone()) {
                    Some(core) => {
                        let _ = core.destroy_object(proxy);
                        let _ = reply.send(Ok(()));
                    }
                    None => {
                        let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                    }
                }
            } else if let Some(id) = s.adopted_sinks.remove(&name) {
                let _ = registry.destroy_global(id);
                let _ = reply.send(Ok(()));
            } else {
                // Nothing to destroy - idempotent success.
                let _ = reply.send(Ok(()));
            }
        }
        Cmd::ListStreams { reply } => {
            let s = state.borrow();
            let streams = s
                .nodes
                .values()
                .filter(|n| n.media_class == STREAM_CLASS)
                .map(|n| {
                    let (app_name, match_prop, match_value) =
                        crate::audio::types::resolve_identity(|key| n.props.get(key).cloned());
                    let client = n
                        .props
                        .get("client.id")
                        .and_then(|v| v.parse::<u32>().ok())
                        .map(|cid| s.clients.get(&cid).map(|c| (&c.props, c.settled)));
                    let (props, settled) = stream_facts(&n.props, client);
                    AppStream {
                        props,
                        settled,
                        index: n.id,
                        serial: n.serial.unwrap_or_else(|| u64::from(n.id)),
                        app_name,
                        match_prop,
                        match_value,
                        alias: None,
                        icon_name: n.props.get("application.icon-name").cloned(),
                        icon_path: None,
                        pid: n
                            .props
                            .get("application.process.id")
                            .and_then(|v| v.parse().ok()),
                        assigned_sink: s
                            .sink_of_stream(n.id)
                            .and_then(|sink| sink.props.get("node.name"))
                            .filter(|name| is_virtual_sink(name))
                            .cloned(),
                        volume_percent: n.volume_percent,
                        muted: n.muted,
                        active: n.active,
                    }
                })
                .collect();
            let _ = reply.send(Ok(streams));
        }
        Cmd::ListOutputs { reply } => {
            let s = state.borrow();
            let outputs = s
                .nodes
                .values()
                // Where a channel may be sent: real devices only. One of
                // our own nodes would feed itself.
                .filter(|n| {
                    n.media_class == SINK_CLASS
                        && !n
                            .props
                            .get("node.name")
                            .is_some_and(|name| is_own_sink(name))
                })
                .map(|n| OutputDevice {
                    index: n.id,
                    name: n.props.get("node.name").cloned().unwrap_or_default(),
                    description: n
                        .props
                        .get("node.description")
                        .or_else(|| n.props.get("node.nick"))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let _ = reply.send(Ok(outputs));
        }
        Cmd::SetNodeVolumeByName {
            name,
            percent,
            reply,
        } => {
            let mut s = state.borrow_mut();
            if s.desired.contains_key(&name) {
                s.node_levels
                    .entry(name.clone())
                    .or_default()
                    .volume_percent = Some(percent);
            }
            let _ = reply.send(set_props(s.node_by_name(&name), Some(percent), None));
        }
        Cmd::SetMetersActive { active, reply } => {
            let mut s = state.borrow_mut();
            if s.meters_paused == active {
                s.meters_paused = !active;
                for meter in s.meters.values() {
                    meter.set_active(active);
                }
            }
            let _ = reply.send(Ok(()));
        }
        Cmd::SetNodeMuteByName { name, muted, reply } => {
            let mut s = state.borrow_mut();
            if s.desired.contains_key(&name) {
                s.node_levels.entry(name.clone()).or_default().muted = Some(muted);
            }
            let _ = reply.send(set_props(s.node_by_name(&name), None, Some(muted)));
        }
        Cmd::SetNodeVolumeById { id, percent, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.nodes.get(&id), Some(percent), None));
        }
        Cmd::CreateBus { name, label, reply } => {
            let kind = NodeKind::MixSource;
            let mut s = state.borrow_mut();
            if s.bus_sources.contains_key(&name) || s.node_by_name(&name).is_some() {
                s.desired.insert(name, (label, kind)); // adopted - keep alive
                let _ = reply.send(Ok(()));
                return;
            }
            let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                return;
            };
            match create_node_object(&core, &name, &label, kind) {
                Ok(proxy) => {
                    s.desired.insert(name.clone(), (label, kind));
                    s.bus_sources.insert(name.clone(), proxy);
                    // Reply once the node exists, like a channel: a level or
                    // member set right after must find it.
                    s.pending_creates.entry(name).or_default().push(reply);
                }
                Err(e) => {
                    let _ = reply.send(Err(SinkError::Config(format!("create bus: {e}"))));
                }
            }
        }
        Cmd::DestroyBus { name, reply } => {
            let mut s = state.borrow_mut();
            s.desired.remove(&name);
            s.node_levels.remove(&name);
            s.meters.remove(&name);
            s.bus_members.remove(&name);
            s.bus_links.retain(|(bus, _), _| bus != &name);
            s.send_gains.retain(|(bus, _), _| bus != &name);
            s.send_gain_in_links.retain(|(bus, _), _| bus != &name);
            s.bus_member_gains.retain(|(bus, _), _| bus != &name);
            s.send_gain_failed.retain(|(bus, _)| bus != &name);
            s.mix_outputs.remove(&name);
            s.mix_output_links.retain(|(bus, _), _| bus != &name);
            if let Some(levels) = &s.levels {
                levels.release(&name);
            }
            if let Some(proxy) = s.bus_sources.remove(&name) {
                if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                    let _ = core.destroy_object(proxy);
                }
            }
            let _ = reply.send(Ok(()));
        }
        Cmd::SetBusMembers {
            name,
            channels,
            reply,
        } => {
            state
                .borrow_mut()
                .bus_members
                .insert(name, channels.into_iter().collect());
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetBusMemberGain {
            bus_name,
            member,
            percent,
            reply,
        } => {
            let percent = percent.min(crate::commands::routing::MAX_VOLUME);
            {
                let mut s = state.borrow_mut();
                // A deletion can race this queue: re-check both names
                // against the loop's own live state before storing.
                let bus_live = s
                    .desired
                    .get(&bus_name)
                    .is_some_and(|(_, kind)| kind.is_mix());
                let member_live = s.hardware_inputs.contains_key(&member)
                    || s.desired
                        .get(&member)
                        .is_some_and(|(_, kind)| *kind == NodeKind::Channel);
                if !bus_live || !member_live {
                    let _ = reply.send(Err(SinkError::UnknownSink(bus_name)));
                    return;
                }
                let hardware_member = s.hardware_inputs.contains_key(&member);
                let key = (bus_name, member);
                // Touching the fader is the retry signal for a failed build.
                s.send_gain_failed.remove(&key);
                if hardware_member {
                    s.hardware_route_gains.insert(key.clone(), percent);
                }
                if percent == 100 {
                    s.bus_member_gains.remove(&key);
                } else {
                    s.bus_member_gains.insert(key.clone(), percent);
                }
                // Live re-tune - no relink, so a drag never clicks.
                if let Some(handle) = s.send_gains.get(&key) {
                    handle.set_gain_percent(percent);
                }
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetHardwareInput {
            id,
            source_name,
            volume_percent,
            muted,
            reply,
        } => {
            state.borrow_mut().hardware_inputs.insert(
                id,
                (
                    source_name,
                    volume_percent.min(crate::commands::routing::MAX_VOLUME),
                    muted,
                ),
            );
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::RemoveHardwareInput { id, reply } => {
            let mut s = state.borrow_mut();
            s.hardware_inputs.remove(&id);
            s.input_fx.remove(&id);
            s.input_fx_configs.remove(&id);
            s.hardware_route_gains.retain(|(_, input), _| input != &id);
            s.bus_members.values_mut().for_each(|members| {
                members.remove(&id);
            });
            s.bus_member_gains.retain(|(_, input), _| input != &id);
            s.bus_links.retain(|(_, input), _| input != &id);
            s.send_gains.retain(|(_, input), _| input != &id);
            s.send_gain_in_links.retain(|(_, input), _| input != &id);
            s.send_gain_failed.retain(|(_, input)| input != &id);
            drop(s);
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetMixOutputs {
            name,
            outputs,
            reply,
        } => {
            let enabled = outputs
                .into_iter()
                .filter(|binding| binding.enabled)
                .map(|binding| binding.device)
                .collect::<Vec<_>>();
            {
                let mut s = state.borrow_mut();
                if !s.desired.get(&name).is_some_and(|(_, kind)| kind.is_mix()) {
                    let _ = reply.send(Err(SinkError::UnknownSink(name)));
                    return;
                }
                s.mix_outputs.insert(name.clone(), enabled.clone());
                s.mix_output_links
                    .retain(|(mix, output), _| mix != &name || enabled.contains(output));
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelEq {
            sink_name,
            config,
            reply,
        } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            let (needs_create, needs_destroy) = {
                let mut s = state.borrow_mut();
                s.eq_configs.insert(sink_name.clone(), config.clone());
                // Live re-tune: band edits reach the RT thread through the
                // params atomics, no relink and no audible gap.
                if let Some(handle) = s.eq_streams.get(&sink_name) {
                    handle.params.apply(&config);
                }
                let live = s.eq_streams.contains_key(&sink_name);
                (config.enabled && !live, !config.enabled && live)
            };
            if needs_destroy {
                state.borrow_mut().eq_streams.remove(&sink_name);
            } else if needs_create {
                let sink_id = state.borrow().node_by_name(&sink_name).map(|n| n.id);
                if let Some(sink_id) = sink_id {
                    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                        let _ = reply.send(Err(SinkError::Config(
                            "eq chain requires a live core".into(),
                        )));
                        return;
                    };
                    match EqChainHandle::new(&core, &sink_name, sink_id, &config) {
                        Ok(handle) => {
                            state
                                .borrow_mut()
                                .eq_streams
                                .insert(sink_name.clone(), handle);
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            return;
                        }
                    }
                }
                // WaveSink not live yet (e.g. mid-profile-load): the on_node
                // hook builds the chain from eq_configs when it appears.
            }
            // Re-source the channel's links from/to the insert.
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetInputFx { id, fx, reply } => {
            {
                let mut s = state.borrow_mut();
                let fx = fx.clamped();
                // A live chain re-tunes in place; building or dropping one
                // happens in the reconcile, which also relinks the mixes.
                if let Some(handle) = s.input_fx.get(&id) {
                    handle.params.apply(&fx);
                }
                s.input_fx_configs.insert(id, fx);
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::ListInputs { reply } => {
            let s = state.borrow();
            let inputs = s
                .nodes
                .values()
                .filter(|n| n.media_class == SOURCE_CLASS || n.media_class == VIRTUAL_SOURCE_CLASS)
                .map(|n| OutputDevice {
                    index: n.id,
                    name: n.props.get("node.name").cloned().unwrap_or_default(),
                    description: n
                        .props
                        .get("node.description")
                        .or_else(|| n.props.get("node.nick"))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let _ = reply.send(Ok(inputs));
        }
        Cmd::MoveStream {
            id,
            sink_name,
            reply,
        } => {
            let s = state.borrow();
            let Some(metadata) = s.metadata.as_ref() else {
                let _ = reply.send(Err(SinkError::Config(
                    "no default metadata object (is WirePlumber running?)".into(),
                )));
                return;
            };
            // Empty sink name = back to the default device.
            let target = if sink_name.is_empty() {
                s.default_sink_name.clone()
            } else {
                Some(sink_name.clone())
            };
            let serial = target
                .as_deref()
                .and_then(|name| s.node_by_name(name))
                .and_then(|n| n.serial);
            match serial {
                Some(serial) => {
                    metadata.set_property(
                        id,
                        "target.object",
                        Some("Spa:Id"),
                        Some(&serial.to_string()),
                    );
                    // Clear any stale low-level target left by other tools.
                    metadata.set_property(id, "target.node", None, None);
                    let _ = reply.send(Ok(()));
                }
                None => {
                    let _ = reply.send(Err(SinkError::UnknownSink(
                        target.unwrap_or_else(|| "<default>".into()),
                    )));
                }
            }
        }
    }
}

/// A recreated node starts at 100% and unmuted: give it back the level and
/// mute last set on it. A node seen for the first time has nothing saved.
fn restore_level(s: &State, name: &str, id: u32) {
    let Some(level) = s.node_levels.get(name).copied() else {
        return;
    };
    if level.volume_percent.is_none() && level.muted.is_none() {
        return;
    }
    if let Err(e) = set_props(s.nodes.get(&id), level.volume_percent, level.muted) {
        eprintln!("wavesink: restoring level of {name} failed: {e}");
    }
}

fn set_props(
    entry: Option<&NodeEntry>,
    volume_percent: Option<u8>,
    mute: Option<bool>,
) -> Result<(), SinkError> {
    let Some(entry) = entry else {
        return Err(SinkError::UnknownSink("node not found".into()));
    };
    // Last line of defence: nothing reaches PipeWire above unity, whatever
    // an old config or profile still carries.
    let volume = volume_percent.map(|p| {
        (
            pods::percent_to_linear(p.min(crate::commands::routing::MAX_VOLUME)),
            entry.channels,
        )
    });
    let bytes = pods::props_pod_bytes(volume, mute)?;
    let pod = pw::spa::pod::Pod::from_bytes(&bytes)
        .ok_or_else(|| SinkError::Config("constructed an invalid pod".into()))?;
    entry
        .proxy
        .set_param(pw::spa::param::ParamType::Props, 0, pod);
    Ok(())
}

/// A stream's facts: node props over client props, except daemon-owned keys
/// which come from the client. `client` is None, known-but-unbound, or present.
fn stream_facts(
    node_props: &HashMap<String, String>,
    client: Option<Option<(&HashMap<String, String>, bool)>>,
) -> (HashMap<String, String>, bool) {
    let mut props: HashMap<String, String> = node_props
        .iter()
        .filter(|(k, _)| !crate::audio::types::daemon_owned(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let Some(Some((client_props, settled))) = client else {
        return (props, true);
    };
    for (k, v) in client_props {
        props.entry(k.clone()).or_insert_with(|| v.clone());
    }
    (props, settled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(id: u32, node_id: u32, dir: &str, channel: Option<&str>) -> PortEntry {
        PortEntry {
            id,
            node_id,
            direction: dir.to_string(),
            channel: channel.map(str::to_string),
            index: None,
        }
    }

    #[test]
    fn resolve_source_prefers_live_eq_playback() {
        assert_eq!(resolve_source(Some(77), 10), 77);
    }

    #[test]
    fn resolve_source_falls_back_to_channel() {
        assert_eq!(resolve_source(None, 10), 10);
    }

    #[test]
    fn desired_pairs_matches_stereo_by_channel_not_index() {
        let mut s = State::default();
        // Monitor FL/FR on node 10, inputs FL/FR on node 20 with ids ordered
        // so a naive index pairing would cross the channels.
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        s.ports.insert(2, port(2, 10, "out", Some("FR")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        s.ports.insert(4, port(4, 20, "in", Some("FL")));
        let mut pairs = desired_pairs(&s, 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(1, 4), (2, 3)]);
    }

    #[test]
    fn desired_pairs_unnamed_ports_follow_node_order_not_global_ids() {
        // A pro-audio device exposes AUX0..AUXn, so channel names never match.
        // The monitor's FR registered first (lower global id); pairing must
        // still send FL to AUX0 and FR to AUX1.
        let indexed = |id, node, dir: &str, ch: &str, index| PortEntry {
            index: Some(index),
            ..port(id, node, dir, Some(ch))
        };
        let mut s = State::default();
        s.ports.insert(5, indexed(5, 10, "out", "FR", 1));
        s.ports.insert(6, indexed(6, 10, "out", "FL", 0));
        s.ports.insert(1, indexed(1, 20, "in", "AUX0", 0));
        s.ports.insert(2, indexed(2, 20, "in", "AUX1", 1));
        s.ports.insert(3, indexed(3, 20, "in", "AUX2", 2));
        let mut pairs = desired_pairs(&s, 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(5, 2), (6, 1)]);
    }

    #[test]
    fn desired_pairs_fans_mono_source_to_every_input() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("MONO")));
        s.ports.insert(2, port(2, 20, "in", Some("FL")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        let mut pairs = desired_pairs(&s, 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(1, 2), (1, 3)]);
    }

    #[test]
    fn a_mix_is_a_capturable_virtual_source() {
        assert_eq!(NodeKind::MixSource.media_class(), VIRTUAL_SOURCE_CLASS);
        assert!(NodeKind::MixSource.is_mix() && !NodeKind::Channel.is_mix());
    }

    #[test]
    fn desired_pairs_empty_for_self_or_missing_ports() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        assert!(desired_pairs(&s, 10, 10).is_empty(), "same node");
        assert!(desired_pairs(&s, 10, 20).is_empty(), "target has no inputs");
    }

    #[test]
    fn fallback_picks_highest_priority_real_sink() {
        let candidates = [
            (1u32, "sink_game", 10_000i64), // virtual - never a fallback
            (2, "alsa_output.hdmi", 500),
            (3, "alsa_output.analog", 900),
            (4, "alsa_output.usb", 700),
        ];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), Some(3));
    }

    #[test]
    fn fallback_is_none_when_only_our_own_sinks_exist() {
        // Pins: routing a channel into one of our own nodes must never win - a
        // mix already receives every channel, so that would loop.
        let candidates = [
            (1u32, "sink_game", 0i64),
            (2, "sink_chat", 0),
            (3, "sink_stream", 0),
            (4, "sink_bus_voice_only", 0),
        ];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), None);

        // A real device among them still wins.
        let candidates = [
            (1u32, "sink_game", 0i64),
            (3, "sink_stream", 0),
            (9, "alsa_output.pci-0000_00_1f.3.analog-stereo", 1000),
        ];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), Some(9));
    }

    fn kv(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_stream_cannot_forge_the_daemon_owned_props() {
        let node = kv(&[
            ("application.name", "evil"),
            ("pipewire.sec.pid", "1"),
            ("pipewire.access", "unrestricted"),
            ("application.process.id", "1"),
        ]);
        let client = kv(&[
            ("pipewire.sec.pid", "4242"),
            ("pipewire.access", "flatpak"),
            ("pipewire.access.portal.app_id", "com.example.App"),
            ("application.name", "client-name"),
        ]);
        let (props, settled) = stream_facts(&node, Some(Some((&client, true))));
        assert!(settled);
        assert_eq!(props["pipewire.sec.pid"], "4242");
        assert_eq!(props["pipewire.access"], "flatpak");
        assert_eq!(props["pipewire.access.portal.app_id"], "com.example.App");
        // Everything else: the node still wins over the client.
        assert_eq!(props["application.name"], "evil");
        assert_eq!(props["application.process.id"], "1");
    }

    #[test]
    fn daemon_owned_props_never_survive_without_a_client() {
        let node = kv(&[("pipewire.sec.pid", "1"), ("application.name", "x")]);
        let (props, settled) = stream_facts(&node, None);
        assert!(settled);
        assert!(!props.contains_key("pipewire.sec.pid"));
        let (props, settled) = stream_facts(&node, Some(None));
        assert!(settled, "a client that never bound will not send more");
        assert!(!props.contains_key("pipewire.sec.pid"));
    }

    #[test]
    fn an_unsettled_client_leaves_the_stream_unsettled() {
        let node = kv(&[("application.name", "x")]);
        let client = kv(&[]);
        let (_, settled) = stream_facts(&node, Some(Some((&client, false))));
        assert!(!settled);
    }
}
