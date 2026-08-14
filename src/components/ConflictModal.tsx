"use client";

import { useEffect, useState } from "react";
import type { PortConflict } from "@/types";
import { api } from "@/lib/tauri";

interface Props {
  conflicts: PortConflict[];
  onKillAndContinue: () => void;
  onUseAltPort: (overrides: Record<number, number>) => void;
  onCancel: () => void;
  busy: boolean;
}

export default function ConflictModal({
  conflicts,
  onKillAndContinue,
  onUseAltPort,
  onCancel,
  busy,
}: Props) {
  const [overrides, setOverrides] = useState<Record<number, string>>({});

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const next: Record<number, string> = {};
      for (const c of conflicts) {
        try {
          const free = await api.findFreePort(c.port + 1);
          if (!cancelled) next[c.stepIndex] = String(free);
        } catch {
          if (!cancelled) next[c.stepIndex] = String(c.port + 1);
        }
      }
      if (!cancelled) setOverrides(next);
    })();
    return () => {
      cancelled = true;
    };
  }, [conflicts]);

  const allValid =
    conflicts.length > 0 &&
    conflicts.every((c) => {
      const v = Number(overrides[c.stepIndex]);
      return Number.isInteger(v) && v >= 1 && v <= 65535;
    });

  function handleAltPort() {
    const map: Record<number, number> = {};
    for (const c of conflicts) {
      const v = Number(overrides[c.stepIndex]);
      if (Number.isInteger(v) && v >= 1 && v <= 65535) {
        map[c.stepIndex] = v;
      }
    }
    onUseAltPort(map);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="w-full max-w-md rounded-lg bg-white p-5 shadow-xl">
        <h2 className="mb-1 text-lg font-semibold text-zinc-900">
          Bentrok Port Terdeteksi
        </h2>
        <p className="mb-4 text-sm text-zinc-600">
          Port berikut sudah dipakai proses lain. Pilih port alternatif atau
          kill proses yang bentrok:
        </p>

        <div className="mb-4 flex flex-col gap-2">
          {conflicts.map((c, i) => (
            <div
              key={i}
              className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="min-w-0">
                  <span className="font-mono font-semibold text-amber-800">
                    Port {c.port}
                  </span>
                  <span className="text-amber-700">
                    {" "}
                    — {c.processNames.join(", ") || "proses tidak dikenal"}
                  </span>
                  <div className="mt-0.5 text-xs text-amber-600">
                    PID: {c.pids.join(", ")}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <label className="text-xs text-zinc-600">ganti ke</label>
                  <input
                    type="number"
                    min={1}
                    max={65535}
                    disabled={busy}
                    value={overrides[c.stepIndex] ?? ""}
                    onChange={(e) =>
                      setOverrides((prev) => ({
                        ...prev,
                        [c.stepIndex]: e.target.value,
                      }))
                    }
                    className="w-24 rounded-md border border-zinc-300 px-2 py-1 text-sm"
                  />
                </div>
              </div>
            </div>
          ))}
        </div>

        <div className="flex flex-col gap-2">
          <button
            onClick={handleAltPort}
            disabled={busy || !allValid}
            className="w-full rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            Ganti Port &amp; Mulai
          </button>
          <button
            onClick={onKillAndContinue}
            disabled={busy}
            className="w-full rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50"
          >
            Kill &amp; Lanjutkan
          </button>
          <button
            onClick={onCancel}
            disabled={busy}
            className="w-full rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50 disabled:opacity-50"
          >
            Batal
          </button>
        </div>
      </div>
    </div>
  );
}
