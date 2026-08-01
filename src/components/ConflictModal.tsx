"use client";

import type { PortConflict } from "@/types";

interface Props {
  conflicts: PortConflict[];
  onKillAndContinue: () => void;
  onUseAltPort: () => void;
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
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="w-full max-w-md rounded-lg bg-white p-5 shadow-xl">
        <h2 className="mb-1 text-lg font-semibold text-zinc-900">
          Bentrok Port Terdeteksi
        </h2>
        <p className="mb-4 text-sm text-zinc-600">
          Port berikut sudah dipakai proses lain:
        </p>

        <div className="mb-4 flex flex-col gap-2">
          {conflicts.map((c, i) => (
            <div
              key={i}
              className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm"
            >
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
          ))}
        </div>

        <div className="flex flex-col gap-2">
          <button
            onClick={onKillAndContinue}
            disabled={busy}
            className="w-full rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:opacity-50"
          >
            Kill &amp; Lanjutkan
          </button>
          <button
            onClick={onUseAltPort}
            disabled={busy}
            className="w-full rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            Ganti Port Otomatis
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
