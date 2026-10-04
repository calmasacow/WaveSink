# Testing and manual verification

## Automated checks

```bash
npm ci
npm run build
npm test -- --run
cd src-tauri
cargo check
cargo test
```

`cargo check` verifies the Tauri/Rust compilation. `cargo test` exercises the
mock backend and persistence tests; it does not prove the live PipeWire graph.

## Live PipeWire smoke test

Run the app with a working PipeWire and WirePlumber session:

```bash
npm run tauri dev
```

Then verify:

1. Start a game, Discord, browser, and Spotify. Assign each app to a software
   input and restart WaveSink; assignments must persist.
2. Set different Personal and Stream sends. Move the Personal source fader;
   both destinations should change proportionally, but changing a cell send
   must affect only its destination.
3. Select Personal as the monitor, then Stream. The listened mix must change
   without changing app assignments.
4. Select `WaveSink: Stream` in OBS and `WaveSink: Chat` in Discord. Neither should
   require Desktop Audio.
5. Mute Music on Stream and confirm it remains audible in Personal.
6. Add a new app/input and confirm Chat does not receive it automatically.
7. Turn on a hardware input's Audio FX (gate/compressor/limiter) and confirm
   the processed signal reaches every mix that input is routed to, and that
   turning every stage off returns it to a direct link.
8. Destroy a custom mix and inspect the PipeWire graph for orphaned null sinks,
   loopbacks, or monitor links.
9. Switch profiles and verify source levels, cell sends/mutes, mix selection,
   and output bindings restore together.

Useful inspection commands:

```bash
wpctl status
pw-dump > /tmp/wavesink-pw-dump.json
pw-link -l
```

The UI and command layer can pass without proving realtime routing. Always
run the live checklist before calling a graph milestone complete.
