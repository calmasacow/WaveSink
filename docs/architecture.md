# Current architecture and migration boundary

## Runtime flow

```text
Tauri commands / tray
          │
          ▼
      AppState
          │
          ├── MixerState
          │     ├── legacy channels/buses (compatibility projection)
          │     └── RoutingModel (new matrix state)
          │
          └── AudioBackend trait
                    ├── PipeWireBackend ──► dedicated PipeWire loop thread
                    └── PactlBackend     ──► fallback subprocess path
```

The native backend owns PipeWire objects on one loop thread. Commands must
talk through `AudioBackend`; they should not manipulate PipeWire proxies from
Tauri command threads.

## Existing graph pieces worth reusing

`src-tauri/src/audio/pw_native/thread.rs` already has:

- virtual channel sink creation/destruction
- bus source/sink creation and membership links
- per-member send-gain inserts
- monitor links
- link policing and external-node healing
- level/meter registration
- per-input Audio FX and per-channel EQ insert lifetimes

The safest next graph change is to extend these link registries rather than
introduce a second PipeWire loop or userspace audio-copy path.

## State ownership

The desired direction is:

```text
RoutingModel ── source of truth for matrix semantics
      │
      ├── persisted routing.json
      ├── profile.routing
      └── compatibility projection into Buses/Channels while migration runs
```

When adding a command, update the model first, then apply the corresponding
backend operation, and make failure recovery refresh the model from Rust.

## Config files

| File | Current purpose |
|---|---|
| `channels.json` | Legacy software-input definitions and source fader state |
| `buses.json` | Legacy mix definitions and native bus compatibility state |
| `outputs.json` | Legacy channel output choices/failover |
| `eq.json` | Existing channel EQ configuration |
| `profiles/*.json` | Profiles; now includes optional `routing` |
| `routing.json` | New input×mix matrix contract |

Keep the legacy files until the graph migration is complete. They are still
used by older commands, profile compatibility, and the fallback backend.
