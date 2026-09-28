import { beforeEach, describe, expect, it } from "vitest";

import { THEMES, bootTheme, useTheme } from "./theme";

const STORAGE_KEY = "wavesink-theme";

function boot() {
  bootTheme();
  return document.documentElement.dataset.theme;
}

describe("theme", () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset.theme;
    useTheme.setState({ theme: "original", omarchy: null });
  });

  it("every theme carries a label and three swatch colours", () => {
    for (const t of THEMES) {
      expect(t.label).not.toBe("");
      expect(t.swatch).toHaveLength(3);
      for (const c of t.swatch) expect(c).toMatch(/^#[0-9a-f]{6}$/i);
    }
    expect(new Set(THEMES.map((t) => t.id)).size).toBe(THEMES.length);
  });

  it("leaves the attribute off for the default theme", () => {
    localStorage.setItem(STORAGE_KEY, "original");
    expect(boot()).toBeUndefined();
  });

  it("restores any theme in the list", () => {
    for (const t of THEMES.filter((t) => t.id !== "original")) {
      localStorage.setItem(STORAGE_KEY, t.id);
      expect(boot()).toBe(t.id);
    }
  });

  // A theme can be dropped between releases; a stale id must not stick.
  it("falls back to the default for an unknown saved theme", () => {
    localStorage.setItem(STORAGE_KEY, "catppuccin-mocha");
    expect(boot()).toBeUndefined();
  });

  it("setTheme applies and persists", () => {
    useTheme.getState().setTheme("gruvbox-dark");
    expect(document.documentElement.dataset.theme).toBe("gruvbox-dark");
    expect(localStorage.getItem(STORAGE_KEY)).toBe("gruvbox-dark");

    useTheme.getState().setTheme("original");
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("migrates the legacy theme key", () => {
    localStorage.setItem("sink-theme", "tokyo-night");
    expect(boot()).toBe("tokyo-night");
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
