import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "1.1.2" }));
vi.mock("../../store/mixer", () => ({
  useMixerStore: (select: (state: unknown) => unknown) => select({ replayOnboarding: () => {} }),
}));
vi.mock("../../store/theme", () => ({
  useTheme: () => ({ theme: "dark", omarchy: null, setTheme: () => {} }),
  themeOptions: () => [{ id: "dark", label: "Dark", swatch: [] }],
}));
vi.mock("./HotkeysSection", () => ({ HotkeysSection: () => null }));
import { SettingsScreen } from "./SettingsScreen";

it("enables a removed panel and requires both confirmations before restoring the OS panel", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  let enabled = false;
  invoke.mockImplementation(async (command, args) => {
    if (command === "set_omarchy_panel") enabled = args.enabled;
    if (command === "get_omarchy_panel" || command === "set_omarchy_panel")
      return { supported: true, enabled };
    if (command === "get_prefs") return { start_minimized: false };
    return false;
  });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const click = async (text: string) => {
    const button = [...host.querySelectorAll("button")].find((b) => b.textContent === text);
    expect(button, text).toBeDefined();
    await act(async () => button!.click());
  };
  try {
    await act(async () => root.render(<SettingsScreen />));
    expect(host.querySelector(".tag")?.textContent).toBe("Inactive");
    expect(host.querySelector('[role="status"]')?.textContent).toContain("not active");
    await click("Enable WaveSink panel");
    expect(invoke).toHaveBeenLastCalledWith("set_omarchy_panel", { enabled: true });
    expect(host.querySelector(".tag")?.textContent).toBe("Active");
    expect(host.querySelector('[role="status"]')?.textContent).toContain("enabled successfully");
    let finish!: (value: unknown) => void;
    invoke.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await click("Reapply panel");
    expect(
      [...host.querySelectorAll("button")].find((b) => b.textContent === "Applying…")?.disabled,
    ).toBe(true);
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Applying");
    await act(async () => finish({ supported: true, enabled: true }));
    expect(host.querySelector('[role="status"]')?.textContent).toContain("reapplied successfully");
    const removals = () =>
      invoke.mock.calls.filter(([cmd, args]) => cmd === "set_omarchy_panel" && !args.enabled);
    await click("Restore OS panel…");
    expect(host.textContent).toContain("This will restore the default OS audio panel.");
    await click("Cancel");
    expect(removals()).toHaveLength(0);
    await click("Restore OS panel…");
    await click("Continue");
    expect(host.querySelector('[role="dialog"]')?.getAttribute("aria-label")).toBe("Are you sure?");
    expect(host.textContent).toContain("Settings > Omarchy and click Enable WaveSink panel");
    await click("Cancel");
    expect(removals()).toHaveLength(0);
    await click("Restore OS panel…");
    await click("Continue");
    await click("Restore OS panel");
    expect(removals()).toHaveLength(1);
    expect(host.querySelector(".tag")?.textContent).toBe("Inactive");
    await click("Enable WaveSink panel");
    expect(invoke).toHaveBeenLastCalledWith("set_omarchy_panel", { enabled: true });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
