import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";

export type InstallKind = "appimage" | "pacman" | "dpkg" | "rpm" | "nix" | "unknown";

export interface UpdateStatus {
  current: string;
  latest: string | null;
  available: boolean;
  install: InstallKind;
  can_self_update: boolean;
  can_launch_update: boolean;
  command: string | null;
  notes_url: string | null;
  checked_at: number | null;
  error: string | null;
}

/** Six hours, as asked: often enough to notice a release, rare enough to ignore. */
export const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const SKIP_KEY = "wavesink-skipped-update";

/** The one version the user asked not to be told about again. */
function savedSkip(): string | null {
  try {
    return localStorage.getItem(SKIP_KEY);
  } catch {
    return null;
  }
}

interface UpdateState {
  status: UpdateStatus | null;
  /** True while a self-update is downloading. */
  installing: boolean;
  /** Set when a self-update fails; the pill shows it. */
  error: string | null;
  /** Tag the user chose to skip; a newer release still alerts. */
  skipped: string | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
  /** Open a terminal that runs the packaged update command. */
  launch: () => Promise<void>;
  /** Remember (or forget) that this tag should not raise an alert. */
  setSkipped: (tag: string, skip: boolean) => void;
}

export const useUpdate = create<UpdateState>((set, get) => ({
  status: null,
  installing: false,
  error: null,
  skipped: savedSkip(),
  setSkipped: (tag, skip) => {
    try {
      if (skip) localStorage.setItem(SKIP_KEY, tag);
      else localStorage.removeItem(SKIP_KEY);
    } catch {
      // A browser with storage blocked still gets the in-session choice.
    }
    set({ skipped: skip ? tag : null });
  },
  check: async () => {
    // A failed check is not worth surfacing on its own; the backend reports it
    // in `status.error` and the pill stays quiet.
    const status = await invoke<UpdateStatus>("check_for_update").catch(() => null);
    if (status) set({ status });
  },
  launch: async () => {
    const tag = get().status?.latest;
    if (!tag) return;
    set({ error: null });
    try {
      // The terminal owns the privilege prompt; this resolves as soon as it
      // opens, not when the update finishes.
      await invoke("launch_update", { tag });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  install: async () => {
    const tag = get().status?.latest;
    if (!tag) return;
    set({ installing: true, error: null });
    try {
      await invoke("perform_update", { tag });
      // On success WaveSink restarts and this window goes away; the check
      // only runs if the relaunch did not take.
      await get().check();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ installing: false });
    }
  },
}));

/**
 * Whether the pill should call attention to itself: an update exists and the
 * user has not skipped that exact version. A later release clears the skip by
 * simply not matching it.
 */
export function alerting(state: Pick<UpdateState, "status" | "skipped">): boolean {
  const { status, skipped } = state;
  return !!status?.available && status.latest !== skipped;
}

/** Check once at startup, then on the interval. Called from main. */
export function bootUpdates() {
  void useUpdate.getState().check();
  window.setInterval(() => void useUpdate.getState().check(), CHECK_INTERVAL_MS);
}
