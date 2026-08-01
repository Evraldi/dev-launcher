"use client";

import { useState } from "react";
import type { Profile, Step } from "@/types";

interface Props {
  initial?: Profile | null;
  onSave: (profile: Profile) => Promise<void>;
  onCancel: () => void;
}

const emptyStep = (): Step => ({
  type: "command",
  cwd: "",
  cmd: "",
  port: null,
});

export default function ProfileForm({ initial, onSave, onCancel }: Props) {
  const [name, setName] = useState(initial?.name ?? "");
  const [steps, setSteps] = useState<Step[]>(
    initial?.steps.length ? initial.steps : [emptyStep()]
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function updateStep(idx: number, patch: Partial<Step>) {
    setSteps((prev) =>
      prev.map((s, i) => (i === idx ? ({ ...s, ...patch } as Step) : s))
    );
  }

  function setStepType(idx: number, type: Step["type"]) {
    setSteps((prev) =>
      prev.map((s, i) => {
        if (i !== idx) return s;
        switch (type) {
          case "exe":
            return { type: "exe", path: (s as { path?: string }).path ?? "" };
          case "open_url":
            return { type: "open_url", url: (s as { url?: string }).url ?? "" };
          case "command":
          default:
            return {
              type: "command",
              cwd: (s as { cwd?: string }).cwd ?? "",
              cmd: (s as { cmd?: string }).cmd ?? "",
              port: (s as { port?: number | null }).port ?? null,
            };
        }
      })
    );
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (!name.trim()) {
      setError("Nama profile wajib diisi.");
      return;
    }
    const validSteps = steps.filter((s) => {
      if (s.type === "exe") return s.path.trim().length > 0;
      if (s.type === "command")
        return s.cwd.trim().length > 0 && s.cmd.trim().length > 0;
      return s.url.trim().length > 0;
    });
    if (validSteps.length === 0) {
      setError("Minimal satu step harus diisi lengkap.");
      return;
    }
    const profile: Profile = {
      id: initial?.id ?? crypto.randomUUID(),
      name: name.trim(),
      steps: validSteps,
    };
    setSaving(true);
    try {
      await onSave(profile);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
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
          onChange={(e) => setName(e.target.value)}
          placeholder="Contoh: Jubelio Clone"
          className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
        />
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-zinc-700">Steps</span>
          <button
            type="button"
            onClick={() => setSteps((prev) => [...prev, emptyStep()])}
            className="rounded-md bg-blue-600 px-2 py-1 text-xs font-medium text-white hover:bg-blue-700"
          >
            + Tambah step
          </button>
        </div>

        {steps.map((step, idx) => (
          <div
            key={idx}
            className="flex flex-col gap-2 rounded-md border border-zinc-200 p-3"
          >
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold text-zinc-500">
                Step {idx + 1}
              </span>
              <div className="flex gap-1">
                <select
                  value={step.type}
                  onChange={(e) => setStepType(idx, e.target.value as Step["type"])}
                  className="rounded-md border border-zinc-300 px-2 py-1 text-xs"
                >
                  <option value="exe">Jalankan Exe</option>
                  <option value="command">Command</option>
                  <option value="open_url">Buka URL</option>
                </select>
                {steps.length > 1 && (
                  <button
                    type="button"
                    onClick={() =>
                      setSteps((prev) => prev.filter((_, i) => i !== idx))
                    }
                    className="rounded-md border border-red-200 px-2 py-1 text-xs text-red-600 hover:bg-red-50"
                  >
                    Hapus
                  </button>
                )}
              </div>
            </div>

            {step.type === "exe" && (
              <input
                value={(step as { path: string }).path}
                onChange={(e) => updateStep(idx, { path: e.target.value })}
                placeholder="Path exe, contoh: C:\laragon\laragon.exe"
                className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
              />
            )}

            {step.type === "command" && (
              <>
                <input
                  value={(step as { cwd: string }).cwd}
                  onChange={(e) => updateStep(idx, { cwd: e.target.value })}
                  placeholder="Working directory, contoh: D:\laragon\www\jubelio-clone"
                  className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                />
                <input
                  value={(step as { cmd: string }).cmd}
                  onChange={(e) => updateStep(idx, { cmd: e.target.value })}
                  placeholder="Perintah, contoh: npm run dev"
                  className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                />
                <input
                  type="number"
                  value={(step as { port: number | null }).port ?? ""}
                  onChange={(e) =>
                    updateStep(idx, {
                      port: e.target.value === "" ? null : Number(e.target.value),
                    })
                  }
                  placeholder="Port (opsional, untuk cek bentrok)"
                  className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
                />
              </>
            )}

            {step.type === "open_url" && (
              <input
                value={(step as { url: string }).url}
                onChange={(e) => updateStep(idx, { url: e.target.value })}
                placeholder="URL, contoh: http://localhost:3000"
                className="w-full rounded-md border border-zinc-300 px-3 py-2 text-sm"
              />
            )}
          </div>
        ))}
      </div>

      {error && (
        <p className="rounded-md bg-red-50 px-3 py-2 text-sm text-red-700">
          {error}
        </p>
      )}

      <div className="flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
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
    </form>
  );
}
