# WaveSink routing model

WaveSink describes audio as inputs feeding independent destination mixes. The Rust
`RoutingModel` is the persisted contract used by the routing table and future
CLI/D-Bus control surfaces. PipeWire remains the realtime graph.

```text
  app stream ──► software input ──┐
  USB mic ─────► hardware input ──┼─ RouteCell(on, send, mute) ──► Mix
  capture card ─► hardware input ─┘          │
                                             ├─► WaveSink: Personal (capture)
                                             ├─► WaveSink: Stream   (capture)
                                             └─► WaveSink: Chat     (capture)
                                                        │
                                            0..N physical output links
```

`InputDef` is a source strip. Its fader and FX are pre-send and feed every
mix. `MixDef` is a destination bus with a master level and zero or more output
bindings (devices, or "System default"). `RouteCell` is the independent input×mix connection;
its enabled state, send level, and mute do not modify another cell or the
source fader.

The model lives in `~/.config/wavesink/routing.json` and is the only source of
truth: a mix carries exactly the inputs whose cells are on. On migration, old
channel, bus, output, mic, EQ and routing JSON files are copied into a
timestamped backup directory beside it. Old channels become software inputs; the old
Stream/Master bus becomes Stream; custom mixes preserve their member gains as
cells. OBS and Discord should select `WaveSink: Stream` and `WaveSink: Chat` instead
of Desktop Audio.
