import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type {
  LogPayload,
  PortConflict,
  PortInfo,
  Profile,
  StartResult,
  StatusPayload,
} from "@/types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error(
      `[dev-launcher] Perintah "${cmd}" hanya bisa dijalankan di dalam aplikasi Tauri.`
    );
  }
  return invoke<T>(cmd, args);
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
  getActivePorts: () => call<PortInfo[]>("get_active_ports"),
  killProcess: (pid: number) => call<void>("kill_process", { pid }),
  checkPortConflicts: (profileId: string) =>
    call<PortConflict[]>("check_port_conflicts", { profileId }),
  startProfile: (profileId: string) =>
    call<StartResult>("start_profile", { profileId }),
  stopProfile: (profileId: string) =>
    call<void>("stop_profile", { profileId }),
};

export async function onProfileLog(
  handler: (payload: LogPayload) => void
): Promise<UnlistenFn> {
  return listen<LogPayload>("profile-log", (e) => handler(e.payload));
}

export async function onProfileStatus(
  handler: (payload: StatusPayload) => void
): Promise<UnlistenFn> {
  return listen<StatusPayload>("profile-status", (e) => handler(e.payload));
}
