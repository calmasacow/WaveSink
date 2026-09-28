# WaveSink Wave Link redesign — OpenCode handoff

This repository is mid-redesign from a Sonar-style channel mixer to a
Wave-Link-style input × mix matrix. The current branch contains the first
working vertical slice: persisted routing data, matrix commands, starter
mixes, and the routing-table UI.

## Verified baseline

Run from the repository root:

```bash
npm ci
npm run build
npm test -- --run
cd src-tauri && cargo check
```

As of 2026-09-25:

- `npm run build`: passes
- `npm test -- --run`: 8 files / 40 tests pass
- `cargo check`: passes
- Cargo warnings were cleaned from the new matrix code

The repository uses Rust 1.88+ and a git PipeWire dependency. The local
machine has Rust 1.98.1 installed through rustup.

## What changed

### Data model

`src-tauri/src/routing_model.rs` defines the new persisted contract:

- `InputDef`: software or hardware source, source fader, mute, FX flags
- `MixDef`: destination bus, master fader/mute, output bindings
- `RouteCell`: independent `enabled`, `send_percent`, and `muted` state
- `RoutingModel`: ordered inputs/mixes, route map, monitor mix, hidden devices

The model is loaded from `~/.config/wavesink/routing.json`. On first migration it
copies the legacy JSON files into a timestamped
`routing-migration-backup-*` directory. Legacy `channels.json`, `buses.json`,
`outputs.json`, `mic.json`, and `eq.json` remain intact.

The graph and object diagram is in [docs/routing-model.md](docs/routing-model.md).

### Backend

`src-tauri/src/commands/matrix.rs` exposes these Tauri commands:

- `get_routing_model`
- `set_route_cell`
- `set_mix_monitor`
- `clear_mix_monitor`
- `set_mix_outputs`
- `set_hidden_devices`
- `set_input_fx`

The current backend materializes cells through the existing native PipeWire
bus/link primitives. `set_route_cell` preserves the separation between the
source fader and the per-destination send. The existing channel/bus commands
are still compatibility projections used by profiles and older UI code.

Initialization creates the historical Stream bus plus Personal and Chat
destination buses. `src-tauri/src/persistence/buses.rs` now permits eight
user mixes.

Profiles now serialize the routing model in addition to legacy channel/bus
fields. Older profiles without `routing` are projected from their legacy
state on load.

### Frontend

`src/components/MixerBoard/RoutingTable.tsx` is now the default Mixer view.
It provides:

- input rows with source fader, meter, mute styling, and FX drawer
- mix columns with independent cell sends and mute controls
- mix monitor selection and physical output selection UI
- compact mixer mode

`src/App.tsx` routes the Mixer tab to this component. The old
`MixerBoard.tsx` remains in the tree as a compatibility/reference surface and
can be removed after the replacement reaches feature parity.

## Important current limitations

These are deliberate next steps, not hidden assumptions:

1. `set_mix_outputs` currently persists output bindings but does not yet build
   multiple physical PipeWire playback links. Existing `set_monitor` still
   monitors one node on the default output.
2. The matrix uses existing channel virtual sinks as software-input
   projections. A full graph migration should replace per-channel playback
   routing with input filter nodes feeding mix buses directly.
3. Hardware inputs beyond the existing processed mic are represented in the
   model/UI contract but are not yet discoverable as first-class matrix rows.
4. The FX drawer persists built-in mic gate/compressor/limiter flags and the
   high-pass setting in the routing model. Only the existing mic DSP chain is
   currently applied in PipeWire; high-pass/de-esser and generic per-input FX
   nodes still need graph work.
5. The physical output picker currently stores one selected binding in the UI;
   multi-select output editing needs to be added.
6. Sound Check, drag reorder, custom mix icon editing, device hiding UI, and
   full matrix profile restore still need dedicated UI work.

Do not describe the current slice as complete Wave Link parity until these
limitations are addressed.

## Recommended next sequence

1. Add a native `set_mix_outputs` backend command that creates/destroys live
   links from each mix source to every enabled physical sink. Reuse the
   existing `channel_links`, `LinkSet`, port discovery, and cleanup/policing
   logic in `src-tauri/src/audio/pw_native/thread.rs`.
2. Make mix destruction remove all source, monitor, and physical-output links;
   add a native integration-style cleanup test or a deterministic mock test.
3. Add hardware-input discovery and a first-class hardware input node/FX chain.
4. Move mic gate/comp/limiter/high-pass processing behind the input model so
   the same processed mic source feeds Personal, Stream, Chat, and custom
   mixes without special cases.
5. Add multi-select output bindings and hidden-device controls to
   `RoutingTable.tsx`.
6. Add Sound Check recording/playback through the monitored mix.
7. Add drag reorder and explicit add/rename/icon actions for inputs and mixes.
8. Extend profile tests to prove the complete matrix round-trips and profile
   switching restores monitor/output bindings.
9. Update the manual acceptance checklist in the user prompt and run it with
   real PipeWire, OBS, Discord, a browser, Spotify, and a game.

## Files to start with

- [docs/routing-model.md](docs/routing-model.md) — model and graph diagram
- [src-tauri/src/routing_model.rs](src-tauri/src/routing_model.rs) — serde model/migration
- [src-tauri/src/commands/matrix.rs](src-tauri/src/commands/matrix.rs) — matrix API
- [src-tauri/src/commands/devices.rs](src-tauri/src/commands/devices.rs) — graph initialization
- [src-tauri/src/audio/pw_native/thread.rs](src-tauri/src/audio/pw_native/thread.rs) — realtime graph
- [src/components/MixerBoard/RoutingTable.tsx](src/components/MixerBoard/RoutingTable.tsx) — matrix UI
- [src/store/mixer.ts](src/store/mixer.ts) — frontend/backend state bridge

## Working rules

- Preserve GPL-3.0 and the existing Tauri + PipeWire + WirePlumber stack.
- Preserve `~/.config/sink` JSON compatibility unless a migration is explicit.
- Keep Rust as the source of truth; TypeScript should render state and invoke
  commands.
- Do not reintroduce channel-owned output routing as the primary primitive.
- Run `cargo check`, `npm run build`, and `npm test -- --run` after backend or
  UI changes.
