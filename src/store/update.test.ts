import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const { useUpdate, CHECK_INTERVAL_MS } = await import("./update");

const STATUS = {
  current: "1.2.1",
  latest: "v1.3.0",
  available: true,
  install: "appimage" as const,
  can_self_update: true,
  command: null,
  notes_url: "https://example.invalid/tag/v1.3.0",
  checked_at: 1,
  error: null,
};

describe("update store", () => {
  beforeEach(() => {
    invoke.mockReset();
    useUpdate.setState({ status: null, installing: false, error: null });
  });

  it("checks every six hours", () => {
    expect(CHECK_INTERVAL_MS).toBe(21_600_000);
  });

  it("stores the status a check returns", async () => {
    invoke.mockResolvedValue(STATUS);
    await useUpdate.getState().check();
    expect(invoke).toHaveBeenCalledWith("check_for_update");
    expect(useUpdate.getState().status?.available).toBe(true);
  });

  it("keeps the last good status when a check fails", async () => {
    invoke.mockResolvedValue(STATUS);
    await useUpdate.getState().check();
    invoke.mockRejectedValue(new Error("offline"));
    await useUpdate.getState().check();
    // A failed check must not blank the pill or claim it is up to date.
    expect(useUpdate.getState().status?.latest).toBe("v1.3.0");
  });

  it("installs the tag it was told about, then re-checks", async () => {
    invoke.mockResolvedValue(STATUS);
    await useUpdate.getState().check();
    invoke.mockClear();
    invoke.mockResolvedValue({ ...STATUS, available: false, latest: "v1.3.0" });
    await useUpdate.getState().install();
    expect(invoke).toHaveBeenCalledWith("perform_update", { tag: "v1.3.0" });
    expect(invoke).toHaveBeenLastCalledWith("check_for_update");
    expect(useUpdate.getState().installing).toBe(false);
  });

  it("reports a failed install and stops spinning", async () => {
    invoke.mockResolvedValue(STATUS);
    await useUpdate.getState().check();
    invoke.mockRejectedValue("checksum mismatch");
    await useUpdate.getState().install();
    expect(useUpdate.getState().error).toContain("checksum mismatch");
    expect(useUpdate.getState().installing).toBe(false);
  });

  it("does nothing when there is no release to install", async () => {
    await useUpdate.getState().install();
    expect(invoke).not.toHaveBeenCalled();
  });
});
