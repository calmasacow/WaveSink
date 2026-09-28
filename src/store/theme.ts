import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";

export type ThemeId = "original" | "tokyo-night" | "gruvbox-dark" | "omarchy";

export const THEMES: { id: Exclude<ThemeId, "omarchy">; label: string; swatch: string[] }[] = [
  { id: "original", label: "Original", swatch: ["#0a0a0b", "#5557e0", "#ededef"] },
  { id: "tokyo-night", label: "Tokyo Night", swatch: ["#1a1b26", "#7aa2f7", "#bb9af7"] },
  { id: "gruvbox-dark", label: "Gruvbox", swatch: ["#282828", "#fe8019", "#b8bb26"] },
];

const STORAGE_KEY = "sink-theme";
const omarchyProperties = [
  "--bg-main",
  "--bg-sidebar",
  "--bg-chat",
  "--bg-surface",
  "--bg-surface-hover",
  "--bg-active",
  "--fg-primary",
  "--fg-secondary",
  "--fg-muted",
  "--accent",
  "--accent-hover",
  "--accent-light",
  "--on-accent",
  "--bg-popover",
  "--bg-popover-hover",
  "--border",
  "--border-popover",
  "--online",
  "--danger",
  "--warning",
  "--danger-rgb",
  "--online-rgb",
  "--tint-channel",
  "--tint-channel-wash",
  "--tint-bus",
  "--tint-bus-wash",
  "--tint-bus-fg",
  "--tint-bus-icon",
];

export interface OmarchyTheme {
  name: string;
  colors: Record<string, string>;
}

function rgb(hex: string) {
  return `${Number.parseInt(hex.slice(1, 3), 16)}, ${Number.parseInt(hex.slice(3, 5), 16)}, ${Number.parseInt(hex.slice(5, 7), 16)}`;
}

function applyOmarchy(theme: OmarchyTheme) {
  const root = document.documentElement;
  const c = theme.colors;
  const values: Record<string, string> = {
    "--bg-main": c.background,
    "--bg-sidebar": c.dark_background ?? c.background,
    "--bg-chat": c.background,
    "--bg-surface": c.lighter_background ?? c.background,
    "--bg-surface-hover": c.selection ?? c.lighter_background ?? c.background,
    "--bg-active": c.selection ?? c.lighter_background ?? c.background,
    "--fg-primary": c.bright_foreground,
    "--fg-secondary": c.foreground,
    "--fg-muted": c.dark_foreground ?? c.foreground,
    "--accent": c.accent,
    "--accent-hover": c.bright_blue ?? c.accent,
    "--accent-light": `color-mix(in srgb, ${c.accent} 12%, transparent)`,
    "--on-accent": c.darker_background ?? c.background,
    "--bg-popover": c.lighter_background ?? c.background,
    "--bg-popover-hover": c.selection ?? c.lighter_background ?? c.background,
    "--border": c.muted ?? c.dark_foreground ?? c.foreground,
    "--border-popover": c.dark_foreground ?? c.foreground,
    "--online": c.green,
    "--danger": c.red,
    "--warning": c.yellow,
    "--danger-rgb": rgb(c.red),
    "--online-rgb": rgb(c.green),
    "--tint-channel": c.selection ?? c.lighter_background ?? c.background,
    "--tint-channel-wash": `color-mix(in srgb, ${c.accent} 7%, transparent)`,
    "--tint-bus": c.brown ?? c.yellow,
    "--tint-bus-wash": `color-mix(in srgb, ${c.yellow} 6%, transparent)`,
    "--tint-bus-fg": c.yellow,
    "--tint-bus-icon": `color-mix(in srgb, ${c.yellow} 12%, transparent)`,
  };
  for (const [property, value] of Object.entries(values)) root.style.setProperty(property, value);
}

function apply(theme: ThemeId, omarchy: OmarchyTheme | null) {
  const root = document.documentElement;
  for (const property of omarchyProperties) root.style.removeProperty(property);
  if (theme === "omarchy" && omarchy) {
    delete root.dataset.theme;
    applyOmarchy(omarchy);
  } else if (theme === "original") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

function initial(): ThemeId {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved === "omarchy" || THEMES.some((t) => t.id === saved)
    ? (saved as ThemeId)
    : "original";
}

interface ThemeState {
  theme: ThemeId;
  omarchy: OmarchyTheme | null;
  setTheme: (theme: ThemeId) => void;
  refreshOmarchy: () => Promise<void>;
}

export const useTheme = create<ThemeState>((set, get) => ({
  theme: initial(),
  omarchy: null,
  setTheme: (theme) => {
    localStorage.setItem(STORAGE_KEY, theme);
    apply(theme, get().omarchy);
    set({ theme });
  },
  refreshOmarchy: async () => {
    const omarchy = await invoke<OmarchyTheme | null>("get_omarchy_theme").catch(() => null);
    const saved = localStorage.getItem(STORAGE_KEY);
    const theme = !saved && omarchy ? "omarchy" : get().theme;
    if (!saved && omarchy) localStorage.setItem(STORAGE_KEY, theme);
    apply(theme, omarchy);
    set({ omarchy, theme });
  },
}));

/** Apply persisted theme before first paint, then discover a live Omarchy palette. */
export function bootTheme() {
  apply(initial(), null);
  void useTheme.getState().refreshOmarchy();
  window.setInterval(() => void useTheme.getState().refreshOmarchy(), 2_000);
}
