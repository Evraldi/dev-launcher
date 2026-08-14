export type Step =
  | { type: "exe"; path: string }
  | { type: "command"; cwd: string; cmd: string; port: number | null }
  | { type: "open_url"; url: string };

export interface Profile {
  id: string;
  name: string;
  folder?: string | null;
  order?: number;
  steps: Step[];
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
}

export interface StepPid {
  stepIndex: number;
  pid: number;
  label: string;
}

export interface StartResult {
  pids: StepPid[];
}

export interface LogPayload {
  profileId: string;
  stepIndex: number;
  source: "stdout" | "stderr" | "system";
  message: string;
}

export interface StatusPayload {
  profileId: string;
  status: "starting" | "running" | "stopped" | "failed";
  message: string;
  stepIndex: number | null;
}

export type ProfileStatus = "stopped" | "running" | "starting" | "failed";

export type StepRuntimeState = "spawned" | "ready" | "timeout";

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
