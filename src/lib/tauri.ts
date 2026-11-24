import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type {
  ConfigStatus,
  LogBatchPayload,
  PreflightResult,
  Profile,
  RuntimeInfo,
  StartAllResult,
  StatusPayload,
  StepStatusPayload,
} from "@/types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Dipakai UI untuk menandai cutoff log tanpa memanggil API waktu di render. */
export function nowMs(): number {
  return Date.now();
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error(
      `[dev-launcher] Perintah "${cmd}" hanya bisa dijalankan di dalam aplikasi Tauri.`
    );
  }
  return invoke<T>(cmd, args);
}

export interface StartProfileOptions {
  portOverrides?: Record<number, number>;
  force?: boolean;
  skipSteps?: number[];
}

export const api = {
  listProfiles: () => call<Profile[]>("list_profiles"),
  listFolders: () => call<string[]>("list_folders"),
  createFolder: (name: string) => call<void>("create_folder", { name }),
  renameFolder: (oldName: string, newName: string) =>
    call<void>("rename_folder", { old: oldName, new: newName }),
  deleteFolder: (name: string) => call<void>("delete_folder", { name }),
  moveProfile: (profileId: string, folder: string | null) =>
    call<void>("move_profile", { profileId, folder }),
  reorderProfiles: (profileIds: string[]) =>
    call<void>("reorder_profiles", { profileIds }),
  saveProfile: (profile: Profile) => call<void>("save_profile", { profile }),
  deleteProfile: (profileId: string) =>
    call<void>("delete_profile", { profileId }),
  getActivePorts: () => call<unknown[]>("get_active_ports"),
  killPortProcess: (pid: number, port: number) =>
    call<void>("kill_port_process", { pid, port }),
  findFreePort: (start: number) =>
    call<number>("find_free_port", { start }),
  preflightProfile: (profileId: string) =>
    call<PreflightResult>("preflight_profile", { profileId }),
  checkPortConflicts: (profileId: string) =>
    call<PreflightResult>("preflight_profile", { profileId }).then(
      (r) => r.conflicts
    ),
  readProfileLog: (profileId: string) =>
    call<string[]>("read_profile_log", { profileId }),
  clearProfileLog: (profileId: string) =>
    call<void>("clear_profile_log", { profileId }),
  startProfile: (profileId: string, options: StartProfileOptions = {}) =>
    call<void>("start_profile", {
      profileId,
      portOverrides: options.portOverrides ?? null,
      force: options.force ?? false,
      skipSteps: options.skipSteps ?? null,
    }),
  startAll: () => call<StartAllResult>("start_all"),
  stopProfile: (profileId: string) =>
    call<void>("stop_profile", { profileId }),
  stopAll: () => call<number>("stop_all"),
  runtimeState: () => call<RuntimeInfo[]>("get_runtime_state"),
  configStatus: () => call<ConfigStatus>("get_config_status"),
  openConfigFile: () => call<void>("open_config_file"),
};

export async function onProfileLogBatch(
  handler: (entries: LogBatchPayload["entries"]) => void
): Promise<UnlistenFn> {
  return listen<LogBatchPayload>("profile-log-batch", (e) =>
    handler(e.payload.entries)
  );
}

export async function onProfileStatus(
  handler: (payload: StatusPayload) => void
): Promise<UnlistenFn> {
  return listen<StatusPayload>("profile-status", (e) => handler(e.payload));
}

export async function onProfileStepStatus(
  handler: (payload: StepStatusPayload) => void
): Promise<UnlistenFn> {
  return listen<StepStatusPayload>("profile-step-status", (e) =>
    handler(e.payload)
  );
}
