export type ShellType = "auto" | "cmd" | "powershell" | "bash";

export type Readiness = "none" | "tcp" | "http";

export type Step =
  | { type: "exe"; path: string }
  | {
      type: "command";
      cwd: string;
      cmd: string;
      port: number | null;
      shell?: ShellType;
      /** Environment variable tambahan untuk proses ini. */
      env?: Record<string, string> | null;
      /** Cara memastikan step siap. Kosong = ikut perilaku lama. */
      readiness?: Readiness | null;
      /** Timeout readiness (ms). */
      readyTimeoutMs?: number | null;
      /** Jeda sebelum step dijalankan (ms). */
      delayMs?: number | null;
      /** Lanjut walau step ini gagal. */
      optional?: boolean | null;
      /** Restart otomatis kalau proses keluar sendiri. */
      autoRestart?: boolean | null;
      maxRestarts?: number | null;
    }
  | { type: "open_url"; url: string };

export interface Profile {
  id: string;
  name: string;
  folder?: string | null;
  order?: number;
  steps: Step[];
  /** Profil yang dikelola dari conf.jsonc (read-only di UI). */
  external?: boolean;
}

export interface ConfigStatus {
  path: string;
  exists: boolean;
  error: string | null;
}

export interface PortInfo {
  port: number;
  protocol: string;
  status: string;
  pid: number;
  processName: string;
  exePath: string;
  cwd: string;
}

export interface PortConflict {
  stepIndex: number;
  port: number;
  pids: number[];
  processNames: string[];
  exePaths: string[];
  cwds: string[];
}

export interface PreflightResult {
  conflicts: PortConflict[];
  errors: string[];
}

export interface RuntimeInfo {
  profileId: string;
  pids: number[];
  startedAt: number;
}

export interface LogPayload {
  profileId: string;
  stepIndex: number;
  source: "stdout" | "stderr" | "system";
  message: string;
}

export interface LogBatchPayload {
  entries: LogPayload[];
}

export interface StatusPayload {
  profileId: string;
  status: "starting" | "running" | "stopped" | "failed";
  message: string;
  stepIndex: number | null;
  /** Kode exit proses terakhir yang selesai (nullable). */
  exitCode?: number | null;
}

export interface StartAllFailure {
  profileId: string;
  name: string;
  error: string;
}

export interface StartAllResult {
  /** Termasuk profile yang sudah berjalan. */
  total: number;
  started: number;
  /** Nama profile yang dilewati karena sudah berjalan. */
  alreadyRunning: string[];
  failed: StartAllFailure[];
}

export type ProfileStatus = "stopped" | "running" | "starting" | "failed";

export type StepRuntimeState = "spawned" | "ready" | "timeout" | "skipped";

export interface StepStatusPayload {
  profileId: string;
  stepIndex: number;
  state: StepRuntimeState;
  port: number | null;
}

export interface LogEntry {
  id: number;
  stepIndex: number;
  source: string;
  message: string;
}
