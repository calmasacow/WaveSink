import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
import { ElgatoControls } from "./ElgatoControls";

const dock = {
  token: "unit-a-connection-1",
  model: "XLR Dock",
  serial: "unit-a",
  gain_db: 49.5,
  max_gain_db: 80,
  muted: false,
  headphone_db: -23.5,
  min_headphone_db: -128,
  phantom: false,
  low_impedance: false,
  monitor_mix: null,
};
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  invoke.mockReset();
  invoke.mockResolvedValue(dock);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function show(sourceName = "dock-a") {
  await act(async () => root.render(<ElgatoControls key={sourceName} sourceName={sourceName} />));
}
function phantom(): HTMLInputElement {
  const label = [...host.querySelectorAll("label")].find((el) =>
    el.textContent?.includes("48 V phantom power"),
  );
  return label!.querySelector("input")!;
}

it("requires confirmation and scopes phantom writes to the displayed connection", async () => {
  await show();
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
  await act(async () => phantom().click());
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(phantom().checked).toBe(false);
  confirm.mockReturnValue(true);
  invoke.mockResolvedValue({ ...dock, phantom: true });
  await act(async () => phantom().click());
  expect(invoke).toHaveBeenLastCalledWith("set_elgato_control", {
    sourceName: "dock-a",
    token: dock.token,
    setting: { control: "phantom", value: true },
    phantomConfirmed: true,
  });
  expect(phantom().checked).toBe(true);
});

it("hides unsupported controls on Wave:3", async () => {
  invoke.mockResolvedValue({
    ...dock,
    model: "Wave:3",
    phantom: null,
    low_impedance: null,
    monitor_mix: 50,
  });
  await show();
  expect(host.textContent).not.toContain("phantom");
  expect(host.textContent).not.toContain("Low impedance");
  expect(host.textContent).toContain("Monitor mix");
});

it("requires refresh after a failed write instead of reusing stale state", async () => {
  await show();
  vi.spyOn(window, "confirm").mockReturnValue(true);
  invoke.mockRejectedValue("Device connection changed");
  await act(async () => phantom().click());
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("Device connection changed");
  expect(host.querySelectorAll('input[type="checkbox"]')).toHaveLength(0);
});

it("ignores reads completing after a device selection change", async () => {
  let finish!: (value: typeof dock) => void;
  invoke.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await show("dock-a");
  invoke.mockResolvedValue({ ...dock, serial: "unit-b", token: "unit-b-connection-1" });
  await show("dock-b");
  await act(async () => finish(dock));
  expect(host.textContent).toContain("unit-b");
  expect(host.textContent).not.toContain("unit-a");
});
