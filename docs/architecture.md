# Architecture

## Runtime flow

```text
Tauri commands / tray
          │
          ▼
      AppState
          │
          ├── MixerState
          │     └── RoutingModel (inputs, mixes, cells: the source of truth)
          │
          └── AudioBackend trait
                    └── PipeWireBackend ──► dedicated PipeWire loop thread
```

The native backend owns PipeWire objects on one loop thread. Commands must
talk through `AudioBackend`; they should not manipulate PipeWire proxies from
Tauri command threads.

## Existing graph pieces worth reusing

`src-tauri/src/audio/pw_native/thread.rs` already has:

- virtual channel sink creation/destruction
- bus source/sink creation and membership links
- per-member send-gain inserts
- link policing and external-node healing
- level/meter registration
- per-input Audio FX and per-channel EQ insert lifetimes

The safest next graph change is to extend these link registries rather than
introduce a second PipeWire loop or userspace audio-copy path.

## State ownership

```text
RoutingModel ── the one source of truth: inputs, mixes, every input×mix cell
      │
      ├── persisted as routing.json (and inside each profile)
      └── applied to PipeWire only through commands/graph.rs
```

A mix's members and send levels are derived from its cells
(`RoutingModel::members`, `member_gain`); nothing stores them separately.
When adding a command, update the model, save it, then apply it through
`commands/graph.rs` (`apply_mix_routes`, `bring_up_mix`, `bring_up`). On a
refused change the UI refetches the model rather than guessing.

## Config files

| File | Purpose |
|---|---|
| `routing.json` | Inputs, mixes, cells, levels, mutes, Audio FX, mix outputs |
| `eq.json` | Per-channel parametric EQ |
| `assignments.json` | App → channel rules |
| `prefs.json` | Preferences (start minimized, meters) |
| `profiles/*.json` | Named snapshots: routing model, assignments, EQ, trigger device |
| `channels.json`, `buses.json`, `outputs.json` | Original Sink layout. Read once to migrate a setup (routing.json version 1 → 2), never written; older profiles carrying them still load |
