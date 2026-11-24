"use client";

import { useEffect, useRef, useState } from "react";
import type { PortConflict } from "@/types";
import { api } from "@/lib/tauri";

interface Props {
  conflicts: PortConflict[];
  onKillAndContinue: () => void;
  onUseAltPort: (overrides: Record<number, number>) => void;
  onSkipExisting: () => void;
  onCancel: () => void;
  busy: boolean;
}

export default function ConflictModal({
  conflicts,
  onKillAndContinue,
  onUseAltPort,
  onSkipExisting,
  onCancel,
  busy,
}: Props) {
  const [overrides, setOverrides] = useState<Record<number, string>>({});
  const userEdited = useRef(new Set<number>());
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    userEdited.current.clear();
    (async () => {
      const next: Record<number, string> = {};
      for (const c of conflicts) {
        const fallback = c.port >= 65535 ? 1 : c.port + 1;
        try {
          const free = await api.findFreePort(fallback);
          next[c.stepIndex] = String(free);
        } catch {
          next[c.stepIndex] = String(fallback);
        }
      }
      // Jangan timpa nilai yang sudah diketik user sebelum hasil async datang.
      if (!cancelled) {
        setOverrides((prev) => {
          const result = { ...next };
          for (const [key, value] of Object.entries(prev)) {
            if (userEdited.current.has(Number(key))) result[Number(key)] = value;
          }
          return result;
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [conflicts]);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && !busy) {
        onCancel();
        return;
      }
      if (e.key !== "Tab" || !dialogRef.current) return;
      const focusable = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), [href], [tabindex]:not([tabindex="-1"])'
        )
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      previous?.focus();
    };
  }, [busy, onCancel]);

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
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      role="presentation"
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="conflict-title"
        className="w-full max-w-lg rounded-lg bg-white p-5 shadow-xl"
      >
        <h2
          id="conflict-title"
          className="mb-1 text-lg font-semibold text-zinc-900"
        >
          Bentrok Port Terdeteksi
        </h2>
        <p className="mb-4 text-sm text-zinc-600">
          Port berikut sudah dipakai proses lain. Pilih ganti port, lewati
          step-nya (biarkan proses lama jalan), atau kill proses yang bentrok.
        </p>

        <div className="mb-4 flex max-h-72 flex-col gap-2 overflow-y-auto">
          {conflicts.map((c, i) => (
            <div
              key={i}
              className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm"
            >
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <span className="font-mono font-semibold text-amber-800">
                    Port {c.port}
                  </span>
                  <span className="text-amber-700">
                    {" "}
                    — {c.processNames.join(", ") || "proses tidak dikenal"}
                  </span>
                  <div className="mt-0.5 text-xs text-amber-700">
                    PID: {c.pids.join(", ")}
                  </div>
                  {c.exePaths.filter(Boolean).length > 0 && (
                    <div
                      className="mt-0.5 truncate text-xs text-amber-600"
                      title={c.exePaths.join("\n")}
                    >
                      {c.exePaths.filter(Boolean).join(", ")}
                    </div>
                  )}
                  {c.cwds.filter(Boolean).length > 0 && (
                    <div
                      className="truncate text-xs text-amber-600"
                      title={c.cwds.join("\n")}
                    >
                      cwd: {c.cwds.filter(Boolean).join(", ")}
                    </div>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <label className="text-xs text-zinc-600">ganti ke</label>
                  <input
                    type="number"
                    min={1}
                    max={65535}
                    disabled={busy}
                    value={overrides[c.stepIndex] ?? ""}
                    onChange={(e) => {
                      userEdited.current.add(c.stepIndex);
                      setOverrides((prev) => ({
                        ...prev,
                        [c.stepIndex]: e.target.value,
                      }));
                    }}
                    className="w-24 rounded-md border border-zinc-300 px-2 py-1 text-sm"
                  />
                </div>
              </div>
            </div>
          ))}
        </div>

        <div className="flex flex-col gap-2">
          <button
            autoFocus
            onClick={handleAltPort}
            disabled={busy || !allValid}
            className="w-full rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus-visible:ring-2 focus-visible:ring-blue-400 disabled:opacity-50"
          >
            Ganti Port &amp; Mulai
          </button>
          <button
            onClick={onSkipExisting}
            disabled={busy}
            className="w-full rounded-md border border-amber-300 bg-amber-50 px-4 py-2 text-sm font-medium text-amber-800 hover:bg-amber-100 disabled:opacity-50"
          >
            Lewati Step Ini &amp; Mulai
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
