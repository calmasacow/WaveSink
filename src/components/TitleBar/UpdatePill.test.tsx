import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const { useUpdate } = await import("../../store/update");
const { UpdatePill } = await import("./UpdatePill");

const BASE = {
  current: "1.2.1",
  latest: "v1.3.0",
  available: true,
  command: null as string | null,
  notes_url: "https://example.invalid/tag/v1.3.0",
  checked_at: Math.floor(Date.now() / 1000),
  error: null,
};

function render() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(<UpdatePill />));
  const click = (text: string) => {
    const el = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text));
    expect(el, text).toBeDefined();
    act(() => el!.click());
  };
  return { host, click };
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  invoke.mockReset();
  localStorage.clear();
  document.body.innerHTML = "";
  useUpdate.setState({ status: null, installing: false, error: null, skipped: null });
});

it("shows nothing until the first check returns", () => {
  const { host } = render();
  expect(host.textContent).toBe("");
});

it("reads as up to date, and re-checks when clicked", () => {
  useUpdate.setState({
    status: { ...BASE, available: false, install: "pacman", can_self_update: false },
  });
  invoke.mockResolvedValue({ ...BASE, available: false });
  const { host, click } = render();
  const pill = host.querySelector("button")!;
  expect(pill.textContent).toContain("v1.2.1");
  expect(pill.className).not.toContain("avail");
  click("v1.2.1");
  expect(invoke).toHaveBeenCalledWith("check_for_update");
});

it("colours the pill and names the version when an update is waiting", () => {
  useUpdate.setState({
    status: { ...BASE, install: "appimage", can_self_update: true },
  });
  const { host } = render();
  const pill = host.querySelector("button")!;
  expect(pill.textContent).toContain("Update 1.3.0");
  expect(pill.className).toContain("avail");
});

it("offers to install an AppImage itself", () => {
  useUpdate.setState({ status: { ...BASE, install: "appimage", can_self_update: true } });
  invoke.mockResolvedValue(BASE);
  const { host, click } = render();
  click("Update 1.3.0");
  expect(host.textContent).toContain("WaveSink v1.3.0 is available");
  click("Update to v1.3.0");
  expect(invoke).toHaveBeenCalledWith("perform_update", { tag: "v1.3.0" });
});

it("gives a packaged install the command instead of an update button", () => {
  useUpdate.setState({
    status: {
      ...BASE,
      install: "pacman",
      can_self_update: false,
      command: "sudo pacman -U https://example.invalid/wavesink.pkg.tar.zst",
    },
  });
  const { host, click } = render();
  click("Update 1.3.0");
  expect(host.querySelector(".modal-code")?.textContent).toContain("sudo pacman -U");
  // The one thing that must never appear: a button implying we can do it.
  const labels = [...host.querySelectorAll("button")].map((b) => b.textContent);
  expect(labels.some((l) => l?.startsWith("Update to"))).toBe(false);
  expect(labels).toContain("Copy command");
});

it("offers the skip box on a packaged install too, and mutes the pill once ticked", () => {
  useUpdate.setState({
    status: {
      ...BASE,
      install: "pacman",
      can_self_update: false,
      command: "sudo pacman -U https://example.invalid/wavesink.pkg.tar.zst",
    },
  });
  const { host, click } = render();
  click("Update 1.3.0");
  const box = host.querySelector<HTMLInputElement>(".modal-check input")!;
  expect(box).toBeDefined();
  expect(box.checked).toBe(false);
  act(() => box.click());
  expect(useUpdate.getState().skipped).toBe("v1.3.0");
  // The pill drops its accent but still reports the release.
  const pill = host.querySelector("button")!;
  expect(pill.className).not.toContain("avail");
  expect(pill.textContent).toContain("Update 1.3.0");
});

it("unticking the box restores the alert", () => {
  useUpdate.setState({
    status: { ...BASE, install: "appimage", can_self_update: true },
    skipped: "v1.3.0",
  });
  const { host, click } = render();
  expect(host.querySelector("button")!.className).not.toContain("avail");
  click("Update 1.3.0");
  const box = host.querySelector<HTMLInputElement>(".modal-check input")!;
  expect(box.checked, "reflects the stored choice").toBe(true);
  act(() => box.click());
  expect(useUpdate.getState().skipped).toBeNull();
  expect(host.querySelector("button")!.className).toContain("avail");
});

it("a newer release alerts even though the previous one was skipped", () => {
  useUpdate.setState({
    status: { ...BASE, latest: "v1.4.0", install: "appimage", can_self_update: true },
    skipped: "v1.3.0",
  });
  const { host } = render();
  const pill = host.querySelector("button")!;
  expect(pill.className).toContain("avail");
  expect(pill.textContent).toContain("Update 1.4.0");
});

it("tells a Nix install to use its own configuration", () => {
  useUpdate.setState({
    status: { ...BASE, install: "nix", can_self_update: false, command: null },
  });
  const { host, click } = render();
  click("Update 1.3.0");
  expect(host.textContent).toContain("managed by Nix");
  expect(host.querySelector(".modal-code")).toBeNull();
});
