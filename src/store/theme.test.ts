import { beforeEach, describe, expect, it } from "vitest";

import { THEMES, bootTheme, resolvedTheme, themeOptions, useTheme } from "./theme";
import { BUNDLED_THEMES } from "../themes/omarchy";

const STORAGE_KEY = "wavesink-theme";
const REQUIRED_COLORS = [
  "accent",
  "background",
  "foreground",
  "bright_foreground",
  "red",
  "yellow",
  "green",
] as const;

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

function boot() {
  bootTheme();
  return document.documentElement.dataset.theme;
}

describe("theme", () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset.theme;
    document.documentElement.removeAttribute("style");
    useTheme.setState({ theme: "original", omarchy: null });
  });

  it("bundles every stock Omarchy palette", () => {
    expect(BUNDLED_THEMES).toHaveLength(22);
    expect(BUNDLED_THEMES.filter((theme) => theme.mode === "light")).toHaveLength(5);
    for (const theme of BUNDLED_THEMES) {
      for (const key of REQUIRED_COLORS) {
        expect(theme.colors[key], `${theme.id}.${key}`).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
  });

  it("keeps primary text readable on every bundled background", () => {
    for (const theme of BUNDLED_THEMES) {
      expect(
        contrast(theme.colors.bright_foreground, theme.colors.background),
        theme.id,
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrast(theme.colors.light_foreground, theme.colors.background),
        `${theme.id} muted`,
      ).toBeGreaterThanOrEqual(3);
    }
  });

  it("every theme carries a label and three swatch colours", () => {
    for (const t of THEMES) {
      expect(t.label).not.toBe("");
      expect(t.description).not.toBe("");
      expect(t.swatch).toHaveLength(3);
      for (const c of t.swatch) expect(c).toMatch(/^#[0-9a-f]{6}$/i);
    }
    expect(new Set(THEMES.map((t) => t.id)).size).toBe(THEMES.length);
  });

  it("shows Follow Omarchy first only when a live palette exists", () => {
    expect(themeOptions(null)[0].id).toBe("original");
    expect(themeOptions(null).some((theme) => theme.id === "omarchy")).toBe(false);

    const options = themeOptions({
      name: "tokyo-night",
      colors: {
        accent: "#7aa2f7",
        background: "#1a1b26",
        bright_foreground: "#c0caf5",
      },
    });
    expect(options[0].id).toBe("omarchy");
    expect(options[0].label).toBe("Follow Omarchy");
    expect(options[0].description).toContain("tokyo-night");
  });

  it("uses platform-aware defaults without overriding manual choices", () => {
    const omarchy = {
      name: "tokyo-night",
      colors: {
        accent: "#7aa2f7",
        background: "#1a1b26",
        bright_foreground: "#c0caf5",
      },
    };
    expect(resolvedTheme(null, "original", omarchy)).toBe("omarchy");
    expect(resolvedTheme(null, "original", null)).toBe("original");
    expect(resolvedTheme("nord", "nord", omarchy)).toBe("nord");
    expect(resolvedTheme("omarchy", "omarchy", null)).toBe("original");
  });

  it("leaves the attribute off for the default theme", () => {
    localStorage.setItem(STORAGE_KEY, "original");
    expect(boot()).toBeUndefined();
  });

  it("restores any theme in the list", () => {
    for (const t of THEMES.filter((t) => t.id !== "original")) {
      localStorage.setItem(STORAGE_KEY, t.id);
      boot();
      expect(document.documentElement.style.getPropertyValue("--bg-main")).toBe(
        BUNDLED_THEMES.find((theme) => theme.id === t.id)?.colors.background,
      );
    }
  });

  it("applies bundled palettes without Omarchy installed", () => {
    localStorage.setItem(STORAGE_KEY, "catppuccin-latte");
    boot();
    expect(document.documentElement.style.getPropertyValue("--bg-main")).toBe("#eff1f5");
    expect(document.documentElement.style.getPropertyValue("--fg-primary")).toBe("#4c4f69");
    expect(document.documentElement.style.colorScheme).toBe("light");
  });

  it("migrates the old Gruvbox theme id", () => {
    localStorage.setItem(STORAGE_KEY, "gruvbox-dark");
    boot();
    expect(localStorage.getItem(STORAGE_KEY)).toBe("gruvbox");
    expect(document.documentElement.style.getPropertyValue("--bg-main")).toBe("#282828");
  });

  // A theme can be dropped between releases; a stale id must not stick.
  it("falls back to the default for an unknown saved theme", () => {
    localStorage.setItem(STORAGE_KEY, "catppuccin-mocha");
    expect(boot()).toBeUndefined();
  });

  it("setTheme applies and persists", () => {
    useTheme.getState().setTheme("gruvbox");
    expect(document.documentElement.style.getPropertyValue("--bg-main")).toBe("#282828");
    expect(localStorage.getItem(STORAGE_KEY)).toBe("gruvbox");

    useTheme.getState().setTheme("original");
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("migrates the legacy theme key", () => {
    localStorage.setItem("sink-theme", "tokyo-night");
    boot();
    expect(document.documentElement.style.getPropertyValue("--bg-main")).toBe("#1a1b26");
    expect(localStorage.getItem(STORAGE_KEY)).toBe("tokyo-night");
    expect(localStorage.getItem("sink-theme")).toBeNull();
  });

  it("uses a detected Omarchy palette when selected", () => {
    useTheme.setState({
      omarchy: {
        name: "tokyo-night",
        colors: {
          accent: "#7aa2f7",
          background: "#1a1b26",
          foreground: "#a9b1d6",
          bright_foreground: "#c0caf5",
          red: "#f7768e",
          yellow: "#e0af68",
          green: "#9ece6a",
        },
      },
    });
    useTheme.getState().setTheme("omarchy");
    expect(document.documentElement.style.getPropertyValue("--accent")).toBe("#7aa2f7");
    expect(localStorage.getItem(STORAGE_KEY)).toBe("omarchy");
  });
});
