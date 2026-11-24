"use client";

import { useEffect, useState } from "react";
import type { Profile, Readiness, ShellType, Step } from "@/types";
import { api } from "@/lib/tauri";
import ConfirmDialog from "@/components/ConfirmDialog";

interface Props {
  initial?: Profile | null;
  onSave: (profile: Profile) => Promise<void>;
  onCancel: () => void;
  /** Dipanggil saat status "ada perubahan belum disimpan" berubah. */
  onDirtyChange?: (dirty: boolean) => void;
}

interface StepDraft {
  uid: string;
  step: Step;
  envText: string;
}

const SHELL_OPTIONS: { value: ShellType; label: string }[] = [
  { value: "auto", label: "Otomatis" },
  { value: "cmd", label: "Command Prompt (cmd)" },
  { value: "powershell", label: "PowerShell" },
  { value: "bash", label: "Bash" },
];

const READINESS_OPTIONS: { value: "" | Readiness; label: string }[] = [
  { value: "", label: "Default (TCP kalau ada port)" },
  { value: "none", label: "Jangan tunggu" },
  { value: "tcp", label: "TCP connect" },
  { value: "http", label: "HTTP request" },
];

function newUid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `step-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function envToText(env?: Record<string, string> | null): string {
  if (!env) return "";
  return Object.entries(env)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

function textToEnv(text: string): Record<string, string> | null {
  const map: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq <= 0) continue;
    const key = line.slice(0, eq).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) continue;
    map[key] = line.slice(eq + 1).trim();
  }
  return Object.keys(map).length > 0 ? map : null;
}

function emptyStep(): Step {
  return {
    type: "command",
    cwd: "",
    cmd: "",
    port: null,
    shell: "auto",
  };
}

function toDraft(step: Step): StepDraft {
  return {
    uid: newUid(),
    step,
    envText: step.type === "command" ? envToText(step.env) : "",
  };
}

function numberOrNull(value: string): number | null {
  if (value.trim() === "") return null;
  const num = Number(value);
  return Number.isFinite(num) ? num : null;
}

function isValidHttpUrl(raw: string): boolean {
  const value = raw.trim();
  if (value.length <= 8 || /[\u0000-\u001f\u007f]/.test(value)) return false;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function hasInvalidNumber(drafts: StepDraft[]): boolean {
  const ranges: Array<[keyof Extract<Step, { type: "command" }>, number, number]> = [
    ["port", 1, 65535],
    ["readyTimeoutMs", 1, 86_400_000],
    ["delayMs", 0, 3_600_000],
    ["maxRestarts", 0, 1000],
  ];
  return drafts.some((draft) =>
    ranges.some(([key, min, max]) => {
      const raw = (draft.step as Record<string, unknown>)[key];
      if (raw === null || raw === undefined || raw === "") return false;
      const value = Number(raw);
      return !Number.isInteger(value) || value < min || value > max;
    })
  );
}

export default function ProfileForm({
  initial,
  onSave,
  onCancel,
  onDirtyChange,
}: Props) {
  const [name, setName] = useState(initial?.name ?? "");
  const [drafts, setDrafts] = useState<StepDraft[]>(() =>
    initial?.steps.length ? initial.steps.map(toDraft) : [toDraft(emptyStep())]
  );
  const [folders, setFolders] = useState<string[]>([]);
  const [folder, setFolder] = useState(initial?.folder ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [showCancelConfirm, setShowCancelConfirm] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await api.listFolders();
        if (!cancelled) setFolders(list);
      } catch {
        // Form tetap bisa dipakai walau folder gagal dimuat.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!dirty) return;
    function onBeforeUnload(e: BeforeUnloadEvent) {
      e.preventDefault();
      e.returnValue = "";
    }
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [dirty]);

  function markDirty() {
    setDirty(true);
    onDirtyChange?.(true);
  }

  function updateStep(uid: string, patch: Record<string, unknown>) {
    setDrafts((prev) =>
      prev.map((d) =>
        d.uid === uid ? { ...d, step: { ...d.step, ...patch } as unknown as Step } : d
      )
    );
    markDirty();
  }

  function updateEnv(uid: string, envText: string) {
    setDrafts((prev) =>
      prev.map((d) => (d.uid === uid ? { ...d, envText } : d))
    );
    markDirty();
  }

  function setStepType(uid: string, type: Step["type"]) {
    setDrafts((prev) =>
      prev.map((d) => {
        if (d.uid !== uid) return d;
        const old = d.step as unknown as Record<string, unknown>;
        let step: Step;
        switch (type) {
          case "exe":
            step = { type: "exe", path: (old.path as string) ?? "" };
            break;
          case "open_url":
            step = { type: "open_url", url: (old.url as string) ?? "" };
            break;
          default:
            step = {
              type: "command",
              cwd: (old.cwd as string) ?? "",
              cmd: (old.cmd as string) ?? "",
              port: (old.port as number | null) ?? null,
              shell: (old.shell as ShellType) ?? "auto",
              env: (old.env as Record<string, string> | null) ?? null,
              readiness: (old.readiness as Readiness | null) ?? null,
              readyTimeoutMs: (old.readyTimeoutMs as number | null) ?? null,
              delayMs: (old.delayMs as number | null) ?? null,
              optional: (old.optional as boolean | null) ?? null,
              autoRestart: (old.autoRestart as boolean | null) ?? null,
              maxRestarts: (old.maxRestarts as number | null) ?? null,
            };
        }
        return { ...d, step };
      })
    );
    markDirty();
  }

  function removeStep(uid: string) {
    setDrafts((prev) => prev.filter((d) => d.uid !== uid));
    markDirty();
  }

  function addStep() {
    setDrafts((prev) => [...prev, toDraft(emptyStep())]);
    markDirty();
  }

  function buildStep(draft: StepDraft): Step {
    const step = draft.step;
    if (step.type === "exe") {
      return { ...step, path: step.path.trim() };
    }
    if (step.type === "open_url") {
      return { ...step, url: step.url.trim() };
    }
    return {
      ...step,
      cwd: step.cwd.trim(),
      env: textToEnv(draft.envText),
      readiness: step.readiness ?? null,
      readyTimeoutMs: step.readyTimeoutMs ?? null,
      delayMs: step.delayMs ?? null,
      optional: step.optional ?? null,
      autoRestart: step.autoRestart ?? null,
      maxRestarts: step.maxRestarts ?? null,
    };
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (!name.trim()) {
      setError("Nama profile wajib diisi.");
      return;
    }
    const validSteps = drafts
      .map(buildStep)
      .filter((s) => {
        if (s.type === "exe") return s.path.trim().length > 0;
        if (s.type === "command")
          return s.cwd.trim().length > 0 && s.cmd.trim().length > 0;
        return s.url.trim().length > 0;
      });
    if (validSteps.length === 0) {
      setError("Minimal satu step harus diisi lengkap.");
      return;
    }
    if (hasInvalidNumber(drafts)) {
      setError(
        "Port harus 1–65535, timeout 1–86400000 ms, delay 0–3600000 ms, maksimal restart 0–1000 (bilangan bulat)."
      );
      return;
    }
    const badUrl = validSteps.find(
      (s) => s.type === "open_url" && !isValidHttpUrl(s.url)
    );
    if (badUrl) {
      setError(
        "Step buka URL hanya menerima alamat http:// atau https:// yang valid."
      );
      return;
    }
    // Folder bisa saja di-rename/dihapus dari sidebar saat form ini terbuka.
    // Cek ulang supaya tidak menyimpan nama folder lama (backend akan membuat
    // folder baru yang tidak terlihat di sidebar).
    let latestFolders = folders;
    try {
      latestFolders = await api.listFolders();
      setFolders(latestFolders);
    } catch {
      // Abaikan: validasi berikutnya memakai daftar yang terakhir diketahui.
    }
    const chosenFolder = folder.trim();
    if (chosenFolder && !latestFolders.includes(chosenFolder)) {
      setError(
        `Folder "${chosenFolder}" sudah tidak ada. Pilih folder lain sebelum menyimpan.`
      );
      return;
    }
    const profile: Profile = {
      id: initial?.id ?? newUid(),
      name: name.trim(),
      folder: chosenFolder || null,
      order: initial?.order ?? 0,
      steps: validSteps,
    };
    setSaving(true);
    try {
      await onSave(profile);
      setDirty(false);
      onDirtyChange?.(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  function closeForm() {
    setShowCancelConfirm(false);
    onDirtyChange?.(false);
    onCancel();
  }

  function handleCancel() {
    if (dirty) {
      setShowCancelConfirm(true);
      return;
    }
    closeForm();
  }

  return (
    <form
      onSubmit={handleSubmit}
      className="flex flex-col gap-4 rounded-lg border border-zinc-200 bg-white p-4 shadow-sm"
    >
      <div>
        <label className="mb-1 block text-sm font-medium text-zinc-700">
          Nama Profile
        </label>
        <input
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            markDirty();
          }}
          placeholder="Contoh: Jubelio Clone"
          className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
        />
      </div>

      <div>
        <label className="mb-1 block text-sm font-medium text-zinc-700">
          Folder
        </label>
        <select
          value={folder}
          onChange={(e) => {
            setFolder(e.target.value);
            markDirty();
          }}
          className="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
        >
          <option value="">Tanpa Folder</option>
          {folders.map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-zinc-700">Steps</span>
          <button
            type="button"
            onClick={addStep}
            className="rounded-md bg-blue-600 px-2 py-1 text-xs font-medium text-white hover:bg-blue-700"
          >
            + Tambah step
          </button>
        </div>

        {drafts.map((draft, idx) => {
          const step = draft.step;
          return (
            <div
              key={draft.uid}
              className="flex flex-col gap-2 rounded-md border border-zinc-200 p-3"
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-zinc-500">
                  Step {idx + 1}
                </span>
                <div className="flex gap-1">
                  <select
                    value={step.type}
                    onChange={(e) =>
                      setStepType(draft.uid, e.target.value as Step["type"])
                    }
                    className="rounded-md border border-zinc-300 px-2 py-1 text-xs"
                  >
                    <option value="exe">Jalankan Exe</option>
                    <option value="command">Command</option>
                    <option value="open_url">Buka URL</option>
                  </select>
                  {drafts.length > 1 && (
                    <button
                      type="button"
                      onClick={() => removeStep(draft.uid)}
                      className="rounded-md border border-red-200 px-2 py-1 text-xs text-red-600 hover:bg-red-50"
                    >
                      Hapus
                    </button>
                  )}
                </div>
              </div>

              {step.type === "exe" && (
                <input
                  value={step.path}
                  onChange={(e) =>
                    updateStep(draft.uid, { path: e.target.value })
                  }
                  placeholder="Path exe, contoh: C:\laragon\laragon.exe"
                  className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                />
              )}

              {step.type === "open_url" && (
                <>
                  <input
                    value={step.url}
                    onChange={(e) =>
                      updateStep(draft.uid, { url: e.target.value })
                    }
                    placeholder="URL, contoh: http://localhost:3000"
                    className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                  />
                  <p className="text-xs text-zinc-400">
                    Hanya URL <code>http://</code> / <code>https://</code> yang
                    diizinkan.
                  </p>
                </>
              )}

              {step.type === "command" && (
                <>
                  <input
                    value={step.cwd}
                    onChange={(e) =>
                      updateStep(draft.uid, { cwd: e.target.value })
                    }
                    placeholder="Working directory, contoh: D:\laragon\www\jubelio-clone"
                    className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                  />
                  <div className="flex gap-2">
                    <select
                      value={step.shell ?? "auto"}
                      onChange={(e) =>
                        updateStep(draft.uid, {
                          shell: e.target.value as ShellType,
                        })
                      }
                      className="w-44 shrink-0 rounded-md border border-zinc-300 bg-white px-2 py-2 text-sm"
                    >
                      {SHELL_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                    <input
                      type="number"
                      min={1}
                      max={65535}
                      value={step.port ?? ""}
                      onChange={(e) =>
                        updateStep(draft.uid, {
                          port: numberOrNull(e.target.value),
                        })
                      }
                      placeholder="Port (opsional, untuk cek bentrok)"
                      className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                    />
                  </div>
                  <textarea
                    value={step.cmd}
                    onChange={(e) =>
                      updateStep(draft.uid, { cmd: e.target.value })
                    }
                    rows={4}
                    spellCheck={false}
                    placeholder={
                      "Perintah, boleh multi-baris. Contoh:\n.\\.env\\Scripts\\Activate.ps1\n$env:ENVIRONMENT=\"development\"\npython main.py"
                    }
                    className="w-full resize-y rounded-md border border-zinc-300 px-3 py-2 font-mono text-sm"
                  />
                  <p className="text-xs text-zinc-400">
                    Multi-baris otomatis ditulis ke script sementara lalu
                    dijalankan oleh shell yang dipilih.
                  </p>

                  <details className="rounded-md border border-zinc-200 bg-zinc-50/60 px-3 py-2">
                    <summary className="cursor-pointer select-none text-xs font-medium text-zinc-600">
                      Pengaturan lanjutan (readiness, env, restart)
                    </summary>
                    <div className="mt-3 flex flex-col gap-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <label className="text-xs text-zinc-600">Readiness</label>
                        <select
                          value={step.readiness ?? ""}
                          onChange={(e) =>
                            updateStep(draft.uid, {
                              readiness:
                                e.target.value === ""
                                  ? null
                                  : (e.target.value as Readiness),
                            })
                          }
                          className="rounded-md border border-zinc-300 bg-white px-2 py-1 text-xs"
                        >
                          {READINESS_OPTIONS.map((o) => (
                            <option key={o.value} value={o.value}>
                              {o.label}
                            </option>
                          ))}
                        </select>
                        <label className="ml-2 text-xs text-zinc-600">
                          Timeout (ms)
                        </label>
                        <input
                          type="number"
                          min={1000}
                          step={500}
                          value={step.readyTimeoutMs ?? ""}
                          onChange={(e) =>
                            updateStep(draft.uid, {
                              readyTimeoutMs: numberOrNull(e.target.value),
                            })
                          }
                          placeholder="30000"
                          className="w-24 rounded-md border border-zinc-300 px-2 py-1 text-xs"
                        />
                        <label className="ml-2 text-xs text-zinc-600">
                          Jeda (ms)
                        </label>
                        <input
                          type="number"
                          min={0}
                          step={100}
                          value={step.delayMs ?? ""}
                          onChange={(e) =>
                            updateStep(draft.uid, {
                              delayMs: numberOrNull(e.target.value),
                            })
                          }
                          placeholder="0"
                          className="w-24 rounded-md border border-zinc-300 px-2 py-1 text-xs"
                        />
                      </div>

                      <div className="flex flex-wrap items-center gap-3">
                        <label className="flex items-center gap-1.5 text-xs text-zinc-600">
                          <input
                            type="checkbox"
                            checked={!!step.optional}
                            onChange={(e) =>
                              updateStep(draft.uid, {
                                optional: e.target.checked,
                              })
                            }
                          />
                          Opsional (lanjut walau gagal)
                        </label>
                        <label className="flex items-center gap-1.5 text-xs text-zinc-600">
                          <input
                            type="checkbox"
                            checked={!!step.autoRestart}
                            onChange={(e) =>
                              updateStep(draft.uid, {
                                autoRestart: e.target.checked,
                              })
                            }
                          />
                          Restart otomatis kalau crash
                        </label>
                        {step.autoRestart && (
                          <label className="flex items-center gap-1.5 text-xs text-zinc-600">
                            Maks.
                            <input
                              type="number"
                              min={0}
                              max={20}
                              value={step.maxRestarts ?? 3}
                              onChange={(e) =>
                                updateStep(draft.uid, {
                                  maxRestarts: numberOrNull(e.target.value),
                                })
                              }
                              className="w-16 rounded-md border border-zinc-300 px-2 py-1 text-xs"
                            />
                            restart
                          </label>
                        )}
                      </div>

                      <div>
                        <label className="mb-1 block text-xs text-zinc-600">
                          Environment variable (satu per baris,{" "}
                          <code>KEY=VALUE</code>)
                        </label>
                        <textarea
                          value={draft.envText}
                          onChange={(e) => updateEnv(draft.uid, e.target.value)}
                          rows={3}
                          spellCheck={false}
                          placeholder={"NODE_ENV=development\nAPI_URL=http://localhost:8080"}
                          className="w-full resize-y rounded-md border border-zinc-300 px-2 py-1 font-mono text-xs"
                        />
                      </div>
                    </div>
                  </details>
                </>
              )}
            </div>
          );
        })}
      </div>

      {error && (
        <p className="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">
          {error}
        </p>
      )}

      <div className="flex justify-end gap-2">
        <button
          type="button"
          onClick={handleCancel}
          className="rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50"
        >
          Batal
        </button>
        <button
          type="submit"
          disabled={saving}
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
        >
          {saving ? "Menyimpan…" : initial ? "Simpan Perubahan" : "Tambah Profile"}
        </button>
      </div>
      {showCancelConfirm && (
        <ConfirmDialog
          title="Buang perubahan?"
          message="Perubahan pada form belum disimpan. Keluar tanpa menyimpan?"
          danger
          confirmLabel="Buang perubahan"
          onConfirm={closeForm}
          onCancel={() => setShowCancelConfirm(false)}
        />
      )}
    </form>
  );
}
