import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useMixerStore, type Levels } from "../store/mixer";
import { pushLevels, setMeterConfig, type MeterUnfocused } from "../lib/meters";

const POLL_INTERVAL_MS = 2000;

/**
 * Boots the audio layer: creates virtual sinks, then keeps app/device state
 * and live levels polled while the window is visible.
 */
export function useAudio() {
  const initialize = useMixerStore((s) => s.initialize);
  const fetchAppStreams = useMixerStore((s) => s.fetchAppStreams);
  const fetchOutputs = useMixerStore((s) => s.fetchOutputs);
  const fetchSeenApps = useMixerStore((s) => s.fetchSeenApps);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const profiles = useMixerStore((s) => s.profiles);
  const loadProfile = useMixerStore((s) => s.loadProfile);

  useEffect(() => {
    void initialize();
    let id: ReturnType<typeof setInterval> | undefined;
    const poll = () => {
      void fetchAppStreams();
      void fetchOutputs();
      void fetchSeenApps();
    };
    const start = () => {
      if (id === undefined) {
        poll(); // refresh immediately so a returning window isn't stale
        id = setInterval(poll, POLL_INTERVAL_MS);
      }
    };
    const stop = () => {
      if (id !== undefined) {
        clearInterval(id);
        id = undefined;
      }
    };
    // Pause polling while hidden in the tray - the product's dominant idle
    // state. Routing stays enforced by a ticker, so this only stops UI refresh.
    const onVisibility = () => (document.hidden ? stop() : start());
    if (!document.hidden) start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [initialize, fetchAppStreams, fetchOutputs, fetchSeenApps]);

  // Meter scale and rates from prefs.
  useEffect(() => {
    void invoke<{ meter_pro: boolean; meter_fps: number; meter_unfocused: MeterUnfocused }>(
      "get_prefs",
    )
      .then((p) =>
        setMeterConfig({ pro: p.meter_pro, fps: p.meter_fps, unfocused: p.meter_unfocused }),
      )
      .catch(() => {});
  }, []);

  // Meters paint straight from the event; nothing re-renders per frame.
  useEffect(() => {
    const unlisten = listen<Levels>("levels", (event) => pushLevels(event.payload));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Profile switched from the tray menu - sync the whole UI.
  const onProfileChanged = useMixerStore((s) => s.onProfileChanged);
  useEffect(() => {
    const unlisten = listen<string>("profile-changed", (event) => {
      void onProfileChanged(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [onProfileChanged]);

  // A hotkey moved the balance in the backend; the strips need the volumes.
  const fetchChannels = useMixerStore((s) => s.fetchChannels);
  useEffect(() => {
    const unlisten = listen("channels-changed", () => void fetchChannels());
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [fetchChannels]);

  // The Omarchy audio panel set a mix level through the CLI.
  const fetchBuses = useMixerStore((s) => s.fetchBuses);
  useEffect(() => {
    const unlisten = listen("buses-changed", () => void fetchBuses());
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [fetchBuses]);

  // Hardware profile auto-switch: when a device with a bound profile
  // appears, load that profile (Sonar-style).
  const seenDevices = useRef<Set<string> | null>(null);
  useEffect(() => {
    const names = new Set(outputDevices.map((d) => d.name));
    if (seenDevices.current === null) {
      // First sample: just learn the current device set.
      if (names.size > 0) seenDevices.current = names;
      return;
    }
    for (const name of names) {
      if (!seenDevices.current.has(name)) {
        const bound = profiles.find((p) => p.trigger_device === name);
        if (bound) void loadProfile(bound.name);
      }
    }
    seenDevices.current = names;
  }, [outputDevices, profiles, loadProfile]);
}
