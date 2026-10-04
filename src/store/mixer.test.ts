import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The store talks to the Rust backend through Tauri IPC; mock the boundary.
const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import { useMixerStore } from "./mixer";
import type { VirtualSink } from "../types";
import { defaultEqConfig } from "../types";

const channel = (name: string, volume = 100): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: volume,
  muted: false,
});

const initialState = useMixerStore.getState();

beforeEach(() => {
  vi.useFakeTimers();
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  useMixerStore.setState(initialState, true);
});

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("setChannelVolume", () => {
  it("updates the UI immediately and debounces the backend call", async () => {
    useMixerStore.setState({ channels: [channel("sink_game")] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_game", 40);
    await store.setChannelVolume("sink_game", 55);

    // Optimistic: the strip moved on the second call already…
    expect(useMixerStore.getState().channels[0].volume_percent).toBe(55);
    // …but the backend hasn't been hit yet (drag in progress).
    expect(invoke).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    // Only the final value of the drag reaches the backend.
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("set_channel_volume", {
      sinkName: "sink_game",
      volume: 55,
    });
  });

  it("keeps per-channel debounce keys separate", async () => {
    useMixerStore.setState({ channels: [channel("sink_game"), channel("sink_chat")] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_game", 10);
    await store.setChannelVolume("sink_chat", 20);
    vi.advanceTimersByTime(100);

    expect(invoke).toHaveBeenCalledTimes(2);
  });
});

describe("setChannelEq", () => {
  it("applies optimistically and debounces per channel", async () => {
    const store = useMixerStore.getState();
    const config = {
      ...defaultEqConfig(),
      enabled: true,
    };

    await store.setChannelEq("sink_game", { ...config, preamp_db: -2 });
    await store.setChannelEq("sink_game", { ...config, preamp_db: -5 });
    await store.setChannelEq("sink_chat", config);

    // Optimistic: both channels reflect their latest config immediately…
    expect(useMixerStore.getState().eqConfigs["sink_game"].preamp_db).toBe(-5);
    expect(useMixerStore.getState().eqConfigs["sink_chat"].enabled).toBe(true);
    // …but nothing has hit the backend yet (drag in progress).
    expect(invoke).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    // One call per channel: sink_game's two edits collapsed into the last.
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenCalledWith("set_channel_eq", {
      sinkName: "sink_game",
      config: { ...config, preamp_db: -5 },
    });
    expect(invoke).toHaveBeenCalledWith("set_channel_eq", {
      sinkName: "sink_chat",
      config,
    });
  });
});
