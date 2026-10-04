import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type {
  AppStream,
  BusDef,
  EqConfig,
  OutputDevice,
  ProfileInfo,
  SeenApp,
  VirtualSink,
  RoutingModel,
  RouteCell,
  FxChain,
} from "../types";

// Faders fire on every pointer move; debounce per target so a drag doesn't
// send a backend command per pixel. UI state updates optimistically.
const pendingInvokes = new Map<string, number>();
function debouncedInvoke(
  key: string,
  cmd: string,
  args: Record<string, unknown>,
  onError: (e: unknown) => void,
) {
  const existing = pendingInvokes.get(key);
  if (existing !== undefined) clearTimeout(existing);
  pendingInvokes.set(
    key,
    window.setTimeout(() => {
      pendingInvokes.delete(key);
      invoke(cmd, args).catch(onError);
    }, 90),
  );
}

/** Per-sink [left, right] peak amplitudes (0-1), streamed from the native backend. */
export type Levels = Record<string, [number, number]>;

interface MixerStore {
  routing: RoutingModel | null;
  fetchRouting: () => Promise<void>;
  setRouteCell: (inputId: string, mixId: string, cell: RouteCell) => Promise<void>;
  setMixOutputs: (mixId: string, devices: string[]) => Promise<void>;
  setInputFx: (inputId: string, fx: FxChain) => Promise<void>;
  setInputLevel: (inputId: string, volume: number, muted: boolean) => Promise<void>;
  updateHardwareInput: (
    inputId: string,
    label: string,
    icon: string,
    iconColor: string,
    sourceName: string,
  ) => Promise<boolean>;
  removeHardwareInput: (inputId: string) => Promise<boolean>;
  addHardwareInput: (
    sourceName: string,
    label: string,
    icon: string | null,
    iconColor: string | null,
  ) => Promise<boolean>;
  channels: VirtualSink[];
  appStreams: AppStream[];
  /** Physical output devices. */
  outputDevices: OutputDevice[];
  fetchOutputs: () => Promise<void>;
  /** Channel -> parametric EQ (absent = never configured, i.e. default). */
  eqConfigs: Record<string, EqConfig>;
  fetchEq: () => Promise<void>;
  setChannelEq: (sinkName: string, config: EqConfig) => Promise<void>;
  /** Hardware capture devices, for adding a hardware input. */
  inputDevices: OutputDevice[];
  fetchInputDevices: () => Promise<void>;
  profiles: ProfileInfo[];
  /** Bind or clear the output device that auto-loads a profile. */
  setProfileTrigger: (name: string, device: string | null) => Promise<void>;
  /** Create a clean-slate profile (saved, not applied). */
  createBlankProfile: (name: string) => Promise<void>;
  /** A profile was switched outside the UI (tray) - sync everything. */
  onProfileChanged: (name: string) => Promise<void>;
  /** App history (live + gone + ignored). */
  seenApps: SeenApp[];
  fetchSeenApps: () => Promise<void>;
  setAppIgnored: (
    app: { match_prop: string; match_value: string },
    ignored: boolean,
  ) => Promise<void>;
  forgetApp: (app: { match_prop: string; match_value: string }) => Promise<void>;
  /** Pre-route an app that isn't currently running (null clears). */
  setAppAssignment: (
    app: { match_prop: string; match_value: string },
    sinkName: string | null,
  ) => Promise<void>;
  /** Channel management: labels are free-form, sink names are stable. */
  addChannel: (label: string, icon: string | null, iconColor?: string | null) => Promise<boolean>;
  renameChannel: (sinkName: string, label: string) => Promise<void>;
  removeChannel: (sinkName: string) => Promise<void>;
  /** Visual-only reorder while dragging a strip. */
  moveChannel: (from: string, to: string) => void;
  /** Persist the current strip order (called on drag end). */
  commitChannelOrder: () => Promise<void>;
  setChannelIcon: (sinkName: string, icon: string) => Promise<void>;
  setChannelIconColor: (sinkName: string, iconColor: string) => Promise<void>;
  /** User-defined mixes (record buses). */
  buses: BusDef[];
  fetchBuses: () => Promise<void>;
  addBus: (label: string) => Promise<void>;
  renameBus: (name: string, label: string) => Promise<void>;
  setBusIcon: (name: string, icon: string) => Promise<void>;
  setBusIconColor: (name: string, iconColor: string) => Promise<void>;
  removeBus: (name: string) => Promise<void>;
  setBusMembers: (name: string, channels: string[]) => Promise<void>;
  /** Manual vs auto-include mode (carried set preserved). */
  setBusExclude: (name: string, exclude: boolean) => Promise<void>;
  /** A mix's playback level for recorders (0-100%); persisted. */
  setBusVolume: (name: string, volume: number) => Promise<void>;
  /** Mute a mix for recorders; persisted. */
  setBusMute: (name: string, muted: boolean) => Promise<void>;
  /** Name of the most recently saved/loaded profile this session. */
  activeProfile: string | null;
  /** Error surfaced to the UI (e.g. a command the backend rejected). */
  error: string | null;
  /** Dismiss the error banner. */
  clearError: () => void;
  initialized: boolean;
  /** First-run tutorial visible. */
  showOnboarding: boolean;
  /** True when the tutorial was reopened from Settings (no setup choice). */
  onboardingReplay: boolean;
  /** Close the tutorial; blank = collapse to a single starter channel. */
  finishOnboarding: (blank: boolean) => Promise<void>;
  /** Reopen the tutorial (view-only - no starting-point choice). */
  replayOnboarding: () => void;

  /** Create the virtual sinks and load initial state. */
  initialize: () => Promise<void>;
  fetchChannels: () => Promise<void>;
  fetchAppStreams: () => Promise<void>;
  setChannelVolume: (sinkName: string, volume: number) => Promise<void>;
  toggleMute: (sinkName: string, muted: boolean) => Promise<void>;
  routeApp: (streamIndex: number, sinkName: string) => Promise<void>;
  setAppVolume: (streamIndex: number, volume: number) => Promise<void>;
  fetchProfiles: () => Promise<void>;
  loadProfile: (name: string) => Promise<void>;
  deleteProfile: (name: string) => Promise<void>;
  /** Set or clear (empty string) a persistent display name for an app. */
  renameApp: (stream: AppStream, alias: string) => Promise<void>;
}

/** Structural equality via JSON, to skip no-op store writes on each poll and
 *  avoid re-rendering the whole board when nothing changed. */
const jsonEqual = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

export const useMixerStore = create<MixerStore>((set, get) => ({
  routing: null,
  fetchRouting: async () => {
    try {
      const routing = await invoke<RoutingModel>("get_routing_model");
      if (!jsonEqual(get().routing, routing)) set({ routing });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  setRouteCell: async (inputId, mixId, cell) => {
    set((s) => ({
      routing: s.routing
        ? {
            ...s.routing,
            routes: {
              ...s.routing.routes,
              [inputId]: { ...s.routing.routes[inputId], [mixId]: cell },
            },
          }
        : null,
    }));
    try {
      await invoke("set_route_cell", {
        inputId,
        mixId,
        enabled: cell.enabled,
        sendPercent: cell.send_percent,
        muted: cell.muted,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchRouting();
    }
  },
  setInputFx: async (inputId, fx) => {
    set((s) => ({
      routing: s.routing
        ? {
            ...s.routing,
            inputs: s.routing.inputs.map((input) =>
              input.id === inputId ? { ...input, fx } : input,
            ),
          }
        : null,
    }));
    try {
      await invoke("set_input_fx", { inputId, fx });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchRouting();
    }
  },
  setInputLevel: async (inputId, volume, muted) => {
    set((s) => ({
      routing: s.routing
        ? {
            ...s.routing,
            inputs: s.routing.inputs.map((input) =>
              input.id === inputId ? { ...input, volume_percent: volume, muted } : input,
            ),
          }
        : null,
    }));
    try {
      await invoke("set_input_level", { inputId, volumePercent: volume, muted });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchRouting();
    }
  },
  updateHardwareInput: async (inputId, label, icon, iconColor, sourceName) => {
    try {
      await invoke("update_hardware_input", { inputId, label, icon, iconColor, sourceName });
      await get().fetchRouting();
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },
  removeHardwareInput: async (inputId) => {
    try {
      await invoke("remove_hardware_input", { inputId });
      await get().fetchRouting();
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },
  addHardwareInput: async (sourceName, label, icon, iconColor) => {
    try {
      await invoke("add_hardware_input", { sourceName, label, icon, iconColor });
      await get().fetchRouting();
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },
  setMixOutputs: async (mixId, devices) => {
    set((s) => ({
      routing: s.routing
        ? {
            ...s.routing,
            mixes: s.routing.mixes.map((mix) =>
              mix.id === mixId
                ? { ...mix, output_bindings: devices.map((device) => ({ device, enabled: true })) }
                : mix,
            ),
          }
        : null,
    }));
    try {
      await invoke("set_mix_outputs", {
        mixId,
        outputs: devices.map((device) => ({ device, enabled: true })),
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchRouting();
    }
  },
  channels: [],
  appStreams: [],
  outputDevices: [],
  inputDevices: [],
  seenApps: [],
  profiles: [],
  activeProfile: null,
  error: null,
  clearError: () => set({ error: null }),
  initialized: false,
  showOnboarding: false,
  onboardingReplay: false,

  replayOnboarding: () => set({ showOnboarding: true, onboardingReplay: true }),

  finishOnboarding: async (blank) => {
    const replay = get().onboardingReplay;
    set({ showOnboarding: false, onboardingReplay: false });
    if (replay) return; // view-only: nothing to persist or change
    try {
      await invoke("set_onboarded");
      if (blank) {
        // Collapse the seeded defaults to a single starter channel; the
        // active profile autosaves the result.
        const channels = get().channels;
        for (const c of channels.slice(1)) {
          await get().removeChannel(c.name);
        }
        if (channels.length > 0) {
          await get().renameChannel(channels[0].name, "Main");
          await get().setChannelIcon(channels[0].name, "graphic_eq");
        }
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  initialize: async () => {
    if (get().initialized) return;
    try {
      await invoke("init_virtual_devices");
      set({ initialized: true, error: null });
      void invoke<{
        onboarded: boolean;
      }>("get_prefs")
        .then((p) => {
          if (!p.onboarded) set({ showOnboarding: true });
        })
        .catch(() => {});
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchProfiles(),
        get().fetchOutputs(),
        get().fetchEq(),
        get().fetchInputDevices(),
        get().fetchBuses(),
        get().fetchRouting(),
      ]);
      // Active profile is tracked backend-side (survives restarts).
      try {
        const active = await invoke<string | null>("get_active_profile");
        if (active) {
          set({ activeProfile: active });
        } else if (get().profiles.some((p) => p.name === "Default")) {
          // First run: the backend just created "Default" from this layout.
          set({ activeProfile: "Default" });
        }
      } catch {
        /* older backend without the command - banner-worthy errors surface elsewhere */
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchChannels: async () => {
    try {
      const channels = await invoke<VirtualSink[]>("get_virtual_devices");
      set({ channels });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchAppStreams: async () => {
    try {
      const appStreams = await invoke<AppStream[]>("get_app_streams");
      const s = get();
      const patch: Partial<MixerStore> = {};
      if (!jsonEqual(s.appStreams, appStreams)) patch.appStreams = appStreams;
      if (s.error !== null) patch.error = null;
      if (Object.keys(patch).length) set(patch);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setChannelVolume: async (sinkName, volume) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, volume_percent: volume } : c)),
    }));
    debouncedInvoke(`chvol:${sinkName}`, "set_channel_volume", { sinkName, volume }, (e) => {
      set({ error: String(e) });
      void get().fetchChannels();
    });
  },

  toggleMute: async (sinkName, muted) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, muted } : c)),
    }));
    try {
      await invoke("toggle_channel_mute", { sinkName, muted });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  routeApp: async (streamIndex, sinkName) => {
    set((s) => {
      // The backend moves every live stream of the app; follow the identity.
      const moved = s.appStreams.find((a) => a.index === streamIndex);
      if (!moved) return {};
      return {
        appStreams: s.appStreams.map((a) =>
          a.match_prop === moved.match_prop && a.match_value === moved.match_value
            ? { ...a, assigned_sink: sinkName === "" ? null : sinkName }
            : a,
        ),
      };
    });
    try {
      await invoke("route_app_to_channel", { streamIndex, sinkName });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      await get().fetchAppStreams();
    }
  },

  setAppVolume: async (streamIndex, volume) => {
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.index === streamIndex ? { ...a, volume_percent: volume } : a,
      ),
    }));
    debouncedInvoke(`appvol:${streamIndex}`, "set_app_volume", { streamIndex, volume }, (e) =>
      set({ error: String(e) }),
    );
  },

  fetchOutputs: async () => {
    try {
      const outputDevices = await invoke<OutputDevice[]>("get_output_devices");
      if (!jsonEqual(get().outputDevices, outputDevices)) set({ outputDevices });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  eqConfigs: {},

  fetchEq: async () => {
    try {
      const eqConfigs = await invoke<Record<string, EqConfig>>("get_channel_eq_configs");
      if (!jsonEqual(get().eqConfigs, eqConfigs)) set({ eqConfigs });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setChannelEq: async (sinkName, config) => {
    set({ eqConfigs: { ...get().eqConfigs, [sinkName]: config } });
    // Debounced per channel: a band drag settles into one apply, and two
    // open EQ panels never clobber each other's pending call.
    debouncedInvoke(`eq:${sinkName}`, "set_channel_eq", { sinkName, config }, (e) => {
      set({ error: String(e) });
      void get().fetchEq();
    });
  },

  fetchInputDevices: async () => {
    try {
      const inputDevices = await invoke<OutputDevice[]>("get_input_devices");
      set({ inputDevices });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchProfiles: async () => {
    try {
      const profiles = await invoke<ProfileInfo[]>("list_profiles");
      set({ profiles });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setProfileTrigger: async (name, device) => {
    try {
      await invoke("set_profile_trigger", { name, device: device ?? "" });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  onProfileChanged: async (name) => {
    set({ activeProfile: name });
    await Promise.all([
      get().fetchChannels(),
      get().fetchAppStreams(),
      get().fetchOutputs(),
      get().fetchEq(),
      get().fetchSeenApps(),
      get().fetchProfiles(),
      get().fetchBuses(),
      get().fetchRouting(),
    ]);
  },

  createBlankProfile: async (name) => {
    try {
      await invoke("create_blank_profile", { name });
      await get().fetchProfiles();
      // Switch to the fresh profile right away - creating a blank slate
      // and not seeing anything change reads as a bug.
      await get().loadProfile(name);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchSeenApps: async () => {
    try {
      const seenApps = await invoke<SeenApp[]>("get_seen_apps");
      if (!jsonEqual(get().seenApps, seenApps)) set({ seenApps });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppIgnored: async (app, ignored) => {
    try {
      await invoke("set_app_ignored", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        ignored,
      });
      await Promise.all([get().fetchSeenApps(), get().fetchAppStreams()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  forgetApp: async (app) => {
    try {
      await invoke("forget_app", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppAssignment: async (app, sinkName) => {
    try {
      await invoke("set_app_assignment", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        sinkName: sinkName ?? "",
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadProfile: async (name) => {
    try {
      await invoke("load_profile", { name });
      set({ activeProfile: name });
      // Layout, volumes and routing all changed backend-side.
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchOutputs(),
        get().fetchEq(),
        get().fetchSeenApps(),
        get().fetchBuses(),
        get().fetchRouting(),
      ]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteProfile: async (name) => {
    try {
      await invoke("delete_profile", { name });
      if (get().activeProfile === name) set({ activeProfile: null });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addChannel: async (label, icon, iconColor = "blue") => {
    try {
      await invoke("add_channel", { label, icon, iconColor });
      // Buses too: the master (and auto-include mixes) absorb the channel.
      await Promise.all([
        get().fetchChannels(),
        get().fetchOutputs(),
        get().fetchBuses(),
        get().fetchRouting(),
      ]);
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },

  buses: [],

  fetchBuses: async () => {
    try {
      const buses = await invoke<BusDef[]>("list_buses");
      set({ buses });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addBus: async (label) => {
    try {
      await invoke("add_bus", { label });
      await get().fetchBuses();
      await get().fetchRouting();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  renameBus: async (name, label) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, label } : b)),
    }));
    try {
      await invoke("rename_bus", { name, label });
      await get().fetchRouting();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
      await get().fetchRouting();
    }
  },
  setBusIcon: async (name, icon) => {
    set((s) => ({ buses: s.buses.map((bus) => (bus.name === name ? { ...bus, icon } : bus)) }));
    try {
      await invoke("set_bus_icon", { name, icon });
      await get().fetchRouting();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },
  setBusIconColor: async (name, iconColor) => {
    set((s) => ({
      buses: s.buses.map((bus) => (bus.name === name ? { ...bus, icon_color: iconColor } : bus)),
    }));
    try {
      await invoke("set_bus_icon_color", { name, iconColor });
      await get().fetchRouting();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
      await get().fetchRouting();
    }
  },

  removeBus: async (name) => {
    try {
      await invoke("remove_bus", { name });
      await get().fetchBuses();
      await get().fetchRouting();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchRouting();
    }
  },

  setBusMembers: async (name, channels) => {
    // `channels` is the carried set; auto-include mixes store the
    // complement (mirrors the backend's conversion).
    const all = get().channels.map((c) => c.name);
    set((s) => ({
      buses: s.buses.map((b) =>
        b.name === name
          ? { ...b, channels: b.exclude ? all.filter((c) => !channels.includes(c)) : channels }
          : b,
      ),
    }));
    try {
      await invoke("set_bus_members", { name, channels });
      // The backend converts against its own channel set - sync up so the
      // stored complement can't drift if channels changed mid-flight.
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  setBusExclude: async (name, exclude) => {
    const all = get().channels.map((c) => c.name);
    set((s) => ({
      buses: s.buses.map((b) => {
        if (b.name !== name || b.exclude === exclude) return b;
        // Preserve the carried set; only the stored representation flips.
        const carried = b.exclude ? all.filter((c) => !b.channels.includes(c)) : b.channels;
        return {
          ...b,
          exclude,
          channels: exclude ? all.filter((c) => !carried.includes(c)) : carried,
        };
      }),
    }));
    try {
      await invoke("set_bus_exclude", { name, exclude });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  setBusVolume: async (name, volume) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, volume_percent: volume } : b)),
    }));
    debouncedInvoke(`busvol:${name}`, "set_bus_volume", { name, volume }, (e) => {
      set({ error: String(e) });
      void get().fetchBuses();
    });
  },

  setBusMute: async (name, muted) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, muted } : b)),
    }));
    try {
      await invoke("set_bus_mute", { name, muted });
      await get().fetchRouting();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
      await get().fetchRouting();
    }
  },

  setChannelIcon: async (sinkName, icon) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, icon } : c)),
    }));
    try {
      await invoke("set_channel_icon", { sinkName, icon });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },
  setChannelIconColor: async (sinkName, iconColor) => {
    set((s) => ({
      channels: s.channels.map((channel) =>
        channel.name === sinkName ? { ...channel, icon_color: iconColor } : channel,
      ),
    }));
    try {
      await invoke("set_channel_icon_color", { sinkName, iconColor });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  renameChannel: async (sinkName, label) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, label } : c)),
    }));
    try {
      await invoke("rename_channel", { sinkName, label });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  // Visual-only move while dragging; commitChannelOrder persists on drop.
  moveChannel: (from, to) => {
    set((s) => {
      const arr = [...s.channels];
      const fi = arr.findIndex((c) => c.name === from);
      const ti = arr.findIndex((c) => c.name === to);
      if (fi < 0 || ti < 0 || fi === ti) return {};
      const [moved] = arr.splice(fi, 1);
      arr.splice(ti, 0, moved);
      return { channels: arr };
    });
  },

  commitChannelOrder: async () => {
    const order = get().channels.map((c) => c.name);
    try {
      await invoke("reorder_channels", { order });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  removeChannel: async (sinkName) => {
    try {
      await invoke("remove_channel", { sinkName });
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchOutputs(),
        get().fetchEq(), // the channel's EQ entry is gone too
        get().fetchBuses(), // memberships dropped the channel
      ]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  renameApp: async (stream, alias) => {
    const trimmed = alias.trim();
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.match_prop === stream.match_prop && a.match_value === stream.match_value
          ? { ...a, alias: trimmed === "" ? null : trimmed }
          : a,
      ),
    }));
    try {
      await invoke("rename_app", {
        matchProp: stream.match_prop,
        matchValue: stream.match_value,
        alias: trimmed,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchAppStreams();
    }
  },
}));
