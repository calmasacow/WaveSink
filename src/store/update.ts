import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";

export type InstallKind = "appimage" | "pacman" | "dpkg" | "rpm" | "nix" | "unknown";

export interface UpdateStatus {
  current: string;
  latest: string | null;
  available: boolean;
  install: InstallKind;
  can_self_update: boolean;
  command: string | null;
  notes_url: string | null;
  checked_at: number | null;
  error: string | null;
}

/** Six hours, as asked: often enough to notice a release, rare enough to ignore. */
export const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

interface UpdateState {
  status: UpdateStatus | null;
  /** True while a self-update is downloading. */
  installing: boolean;
  /** Set when a self-update fails; the pill shows it. */
  error: string | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
}

export const useUpdate = create<UpdateState>((set, get) => ({
  status: null,
  installing: false,
  error: null,
  check: async () => {
    // A failed check is not worth surfacing on its own; the backend reports it
    // in `status.error` and the pill stays quiet.
    const status = await invoke<UpdateStatus>("check_for_update").catch(() => null);
    if (status) set({ status });
  },
  install: async () => {
    const tag = get().status?.latest;
    if (!tag) return;
    set({ installing: true, error: null });
    try {
      await invoke("perform_update", { tag });
      // The new image is in place; it runs on next launch.
      await get().check();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ installing: false });
    }
  },
}));

/** Check once at startup, then on the interval. Called from main. */
export function bootUpdates() {
  void useUpdate.getState().check();
  window.setInterval(() => void useUpdate.getState().check(), CHECK_INTERVAL_MS);
}
