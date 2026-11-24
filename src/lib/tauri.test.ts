import { beforeEach, describe, expect, it, vi } from "vitest";

// vi.hoisted-supaya mock factory (yang di-hoist Vitest) bisa memakainya.
const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: listenMock,
  // Diimpor sebagai tipe oleh tauri.ts; disediakan agar mock tetap lengkap.
  UnlistenFn: undefined,
}));

import { api, isTauri, nowMs, onProfileStatus } from "@/lib/tauri";
import type { StatusPayload } from "@/types";

type StatusListener = (event: { payload: StatusPayload }) => void;

function setTauriFlag(present: boolean): void {
  if (present) {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
      writable: true,
    });
  } else {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  }
}

beforeEach(() => {
  vi.clearAllMocks();
  setTauriFlag(false);
  invokeMock.mockResolvedValue(undefined);
  listenMock.mockResolvedValue(() => undefined);
});

describe("isTauri", () => {
  it("mendeteksi keberadaan __TAURI_INTERNALS__ di window", () => {
    expect(isTauri()).toBe(false);
    setTauriFlag(true);
    expect(isTauri()).toBe(true);
    setTauriFlag(false);
    expect(isTauri()).toBe(false);
  });
});

describe("nowMs", () => {
  it("mengembalikan angka timestamp", () => {
    const before = Date.now();
    const value = nowMs();
    const after = Date.now();
    expect(typeof value).toBe("number");
    expect(value).toBeGreaterThanOrEqual(before);
    expect(value).toBeLessThanOrEqual(after);
  });
});

describe("api", () => {
  it("startProfile meneruskan opsi ke command start_profile", async () => {
    setTauriFlag(true);
    await api.startProfile("p1", {
      portOverrides: { 0: 3001 },
      skipSteps: [1],
    });
    expect(invokeMock).toHaveBeenCalledWith("start_profile", {
      profileId: "p1",
      portOverrides: { 0: 3001 },
      force: false,
      skipSteps: [1],
    });
  });

  it("memanggil command status dan buka konfigurasi", async () => {
    setTauriFlag(true);
    await api.configStatus();
    await api.openConfigFile();
    expect(invokeMock).toHaveBeenNthCalledWith(1, "get_config_status", undefined);
    expect(invokeMock).toHaveBeenNthCalledWith(2, "open_config_file", undefined);
  });

  it("melempar error jelas bila dipanggil di luar Tauri", async () => {
    await expect(api.configStatus()).rejects.toThrow(
      /hanya bisa dijalankan di dalam aplikasi Tauri/
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("onProfileStatus", () => {
  it("meneruskan payload event profile-status ke handler", async () => {
    let eventName: string | undefined;
    let captured: StatusListener | undefined;
    listenMock.mockImplementation(
      async (name: string, listener: StatusListener) => {
        eventName = name;
        captured = listener;
        return () => undefined;
      }
    );

    const handler = vi.fn<(payload: StatusPayload) => void>();
    const unlisten = await onProfileStatus(handler);

    expect(eventName).toBe("profile-status");
    expect(captured).toBeTypeOf("function");

    const payload: StatusPayload = {
      profileId: "p1",
      status: "running",
      message: "",
      stepIndex: 0,
    };
    captured?.({ payload });
    expect(handler).toHaveBeenCalledWith(payload);
    expect(unlisten).toBeTypeOf("function");
  });
});
