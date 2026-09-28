import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { BUNDLED_THEMES, type BundledTheme } from "../themes/omarchy";

export type ThemeId = "original" | "omarchy" | (typeof BUNDLED_THEMES)[number]["id"];

export interface ThemeOption {
  id: ThemeId;
  label: string;
  description: string;
  mode: "dark" | "light";
  swatch: string[];
}

const DESCRIPTIONS: Record<(typeof BUNDLED_THEMES)[number]["id"], string> = {
  catppuccin: "Soft dark pastels with cool lavender-blue accents.",
  "catppuccin-latte": "Airy light pastels with a crisp blue accent.",
  ethereal: "Deep midnight blue with warm cream and violet.",
  everforest: "Muted forest greens with warm natural neutrals.",
  "flexoki-light": "Warm paper tones with restrained ink-blue accents.",
  gruvbox: "Warm, earthy dark tones with softened contrast.",
  hackerman: "Near-black terminal styling with vivid green highlights.",
  kanagawa: "Japanese-inspired ink tones with muted natural colors.",
  "last-horizon": "Near-black charcoal with dusty rose accents.",
  lumon: "Cool blue-gray surfaces with icy cyan highlights.",
  lupine: "Clean light neutrals with saturated blue-violet accents.",
  "matte-black": "Neutral matte black with amber-gold highlights.",
  miasma: "Smoky charcoal with moss, clay, and aged gold.",
  nord: "Arctic blue-gray with calm frost-colored accents.",
  "osaka-jade": "Dark jade greens with warm parchment highlights.",
  "retro-82": "Deep navy with vintage orange and teal accents.",
  ristretto: "Coffee-dark browns with coral and cream highlights.",
  "rose-pine": "Warm light rose neutrals with muted teal accents.",
  solitude: "Minimal charcoal and silver with low-saturation accents.",
  "tokyo-night": "Deep blue night tones with clear blue highlights.",
  vantablack: "Pure black surfaces with monochrome gray contrast.",
  white: "High-contrast white and grayscale minimalism.",
};

export const THEMES: ThemeOption[] = [
  {
    id: "original",
    label: "Original",
    description: "WaveSink's dark graphite palette with indigo accents.",
    mode: "dark",
    swatch: ["#0a0a0b", "#5557e0", "#ededef"],
  },
  ...BUNDLED_THEMES.map((theme): ThemeOption => ({
    ...theme,
    description: DESCRIPTIONS[theme.id],
    swatch: [theme.colors.background, theme.colors.accent, theme.colors.bright_foreground],
  })),
];

export function themeOptions(omarchy: OmarchyTheme | null): ThemeOption[] {
  if (!omarchy) return THEMES;
  return [
    {
      id: "omarchy",
      label: "Follow Omarchy",
      description: `Tracks your current Omarchy desktop palette: ${omarchy.name}.`,
      mode: luminance(omarchy.colors.background) > 0.5 ? "light" : "dark",
      swatch: [omarchy.colors.background, omarchy.colors.accent, omarchy.colors.bright_foreground],
    },
    ...THEMES,
  ];
}

export function resolvedTheme(
  saved: string | null,
  current: ThemeId,
  omarchy: OmarchyTheme | null,
): ThemeId {
  if (!saved) return omarchy ? "omarchy" : "original";
  if (!omarchy && current === "omarchy") return "original";
  return current;
}

const STORAGE_KEY = "wavesink-theme";
const LEGACY_STORAGE_KEY = "sink-theme";
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

function luminance(hex: string) {
  const channels = [1, 3, 5].map((start) => Number.parseInt(hex.slice(start, start + 2), 16) / 255);
  return channels.reduce((sum, value, index) => {
    const linear = value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    return sum + linear * [0.2126, 0.7152, 0.0722][index];
  }, 0);
}

function contrast(a: string, b: string) {
  const [bright, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (bright + 0.05) / (dark + 0.05);
}

function applyPalette(theme: OmarchyTheme | BundledTheme) {
  const root = document.documentElement;
  const c = theme.colors;
  const mode = "mode" in theme ? theme.mode : luminance(c.background) > 0.5 ? "light" : "dark";
  root.style.colorScheme = mode;
  const onAccent = [c.background, c.bright_foreground].sort(
    (a, b) => contrast(b, c.accent) - contrast(a, c.accent),
  )[0];
  const values: Record<string, string> = {
    "--bg-main": c.background,
    "--bg-sidebar": c.dark_background ?? c.background,
    "--bg-chat": c.background,
    "--bg-surface": c.lighter_background ?? c.background,
    "--bg-surface-hover": c.selection ?? c.lighter_background ?? c.background,
    "--bg-active": c.selection ?? c.lighter_background ?? c.background,
    "--fg-primary": c.bright_foreground,
    "--fg-secondary": c.foreground,
    "--fg-muted": c.light_foreground ?? c.foreground,
    "--accent": c.accent,
    "--accent-hover": c.bright_blue ?? c.accent,
    "--accent-light": `color-mix(in srgb, ${c.accent} 12%, transparent)`,
    "--on-accent": onAccent,
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

function bundled(theme: ThemeId): BundledTheme | undefined {
  return BUNDLED_THEMES.find((entry) => entry.id === theme);
}

function apply(theme: ThemeId, omarchy: OmarchyTheme | null) {
  const root = document.documentElement;
  for (const property of omarchyProperties) root.style.removeProperty(property);
  root.style.removeProperty("color-scheme");
  if (theme === "omarchy" && omarchy) {
    delete root.dataset.theme;
    applyPalette(omarchy);
  } else if (theme === "original") delete root.dataset.theme;
  else {
    delete root.dataset.theme;
    const preset = bundled(theme);
    if (preset) applyPalette(preset);
  }
}

function initial(): ThemeId {
  let saved = localStorage.getItem(STORAGE_KEY) ?? localStorage.getItem(LEGACY_STORAGE_KEY);
  if (saved === "gruvbox-dark") saved = "gruvbox";
  if (saved && localStorage.getItem(STORAGE_KEY) !== saved) {
    localStorage.setItem(STORAGE_KEY, saved);
    localStorage.removeItem(LEGACY_STORAGE_KEY);
  }
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
    document.documentElement.toggleAttribute("data-omarchy", omarchy !== null);
    const saved = localStorage.getItem(STORAGE_KEY);
    const current = get().theme;
    const theme = resolvedTheme(saved, current, omarchy);
    if ((!saved && omarchy) || theme !== current) localStorage.setItem(STORAGE_KEY, theme);
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
