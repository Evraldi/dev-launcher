"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { LogEntry } from "@/types";

interface Props {
  entries: LogEntry[];
  activeStepIndex?: number | "all";
  onStepChange?: (step: number | "all") => void;
}

const SOURCE_CLASS: Record<string, string> = {
  stderr: "text-red-400",
  system: "text-sky-400",
  stdout: "text-zinc-300",
};

export default function LogPanel({ entries, activeStepIndex, onStepChange }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const nearBottomRef = useRef(true);
  const copyTimerRef = useRef<number | null>(null);
  const [internalStep, setInternalStep] = useState<number | "all">("all");
  const [autoScroll, setAutoScroll] = useState(true);
  const [wrap, setWrap] = useState(false);
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState(false);

  const isControlled = activeStepIndex !== undefined;
  const activeStep = isControlled ? activeStepIndex : internalStep;
  const setStep = useCallback(
    (step: number | "all") => {
      if (isControlled) {
        onStepChange?.(step);
      } else {
        setInternalStep(step);
      }
    },
    [isControlled, onStepChange]
  );

  const stepIndices = useMemo(
    () =>
      Array.from(new Set(entries.map((e) => e.stepIndex))).sort((a, b) => a - b),
    [entries]
  );

  // Kalau step yang dipilih sudah tidak ada (mis. log dibersihkan), tampilkan All
  // tanpa mengubah state di dalam effect.
  const resolvedStep: number | "all" =
    activeStep !== "all" && !stepIndices.includes(activeStep)
      ? "all"
      : activeStep;

  const filteredEntries = useMemo(() => {
    const byStep =
      resolvedStep === "all"
        ? entries
        : entries.filter((e) => e.stepIndex === resolvedStep);
    const needle = query.trim().toLowerCase();
    if (!needle) return byStep;
    return byStep.filter((e) => e.message.toLowerCase().includes(needle));
  }, [entries, resolvedStep, query]);

  useEffect(() => {
    if (!autoScroll || !nearBottomRef.current) return;
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [filteredEntries.length, autoScroll, resolvedStep]);

  useEffect(
    () => () => {
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
    },
    []
  );

  function formatLines(): string {
    return filteredEntries
      .map((e) => `[step ${e.stepIndex}] ${e.message}`)
      .join("\n");
  }

  async function handleCopy() {
    const text = formatLines();
    let ok = false;
    try {
      await navigator.clipboard.writeText(text);
      ok = true;
    } catch {
      const area = document.createElement("textarea");
      area.value = text;
      document.body.appendChild(area);
      area.select();
      ok = document.execCommand("copy");
      document.body.removeChild(area);
    }
    if (!ok) return;
    setCopied(true);
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
    copyTimerRef.current = window.setTimeout(() => setCopied(false), 1500);
  }

  function handleDownload() {
    const blob = new Blob([formatLines()], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "dev-launcher-log.txt";
    anchor.click();
    URL.revokeObjectURL(url);
  }

  if (entries.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-zinc-500">
        Belum ada log. Tekan Start untuk menjalankan profile.
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-md bg-zinc-950">
      <div className="flex flex-wrap items-center gap-2 border-b border-zinc-800 px-2 py-1.5">
        {stepIndices.length > 1 && (
          <div className="flex flex-wrap gap-1">
            <button
              onClick={() => setStep("all")}
              aria-label="Tampilkan semua step"
              className={`rounded px-2.5 py-1 text-xs font-medium transition-colors ${
                resolvedStep === "all"
                  ? "bg-zinc-700 text-white"
                  : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-300"
              }`}
            >
              All
            </button>
            {stepIndices.map((idx) => (
              <button
                key={idx}
                onClick={() => setStep(idx)}
                aria-label={`Tampilkan step ${idx}`}
                className={`rounded px-2.5 py-1 text-xs font-medium transition-colors ${
                  resolvedStep === idx
                    ? "bg-zinc-700 text-white"
                    : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-300"
                }`}
              >
                Step {idx}
              </button>
            ))}
          </div>
        )}

        <div className="ml-auto flex items-center gap-1.5">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Cari log…"
            aria-label="Cari log"
            className="w-40 rounded border border-zinc-700 bg-zinc-900 px-2 py-1 text-xs text-zinc-200 placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none"
          />
          <button
            onClick={() =>
              setAutoScroll((v) => {
                if (!v) nearBottomRef.current = true;
                return !v;
              })
            }
            aria-pressed={autoScroll}
            aria-label="Aktif/matikan auto-scroll"
            className={`rounded border px-2 py-1 text-xs ${
              autoScroll
                ? "border-emerald-700 bg-emerald-900/40 text-emerald-300"
                : "border-zinc-700 text-zinc-400 hover:bg-zinc-800"
            }`}
            title="Aktif/matikan auto-scroll"
          >
            {autoScroll ? "Auto-scroll: on" : "Auto-scroll: off"}
          </button>
          <button
            onClick={() => setWrap((v) => !v)}
            aria-pressed={wrap}
            aria-label="Aktif/matikan wrap baris log"
            className={`rounded border px-2 py-1 text-xs ${
              wrap
                ? "border-emerald-700 bg-emerald-900/40 text-emerald-300"
                : "border-zinc-700 text-zinc-400 hover:bg-zinc-800"
            }`}
            title="Aktif/matikan wrap baris log"
          >
            {wrap ? "Wrap: on" : "Wrap: off"}
          </button>
          <button
            onClick={handleCopy}
            aria-label="Salin log ke clipboard"
            className="rounded border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:bg-zinc-800"
          >
            {copied ? "Tersalin" : "Salin"}
          </button>
          <button
            onClick={handleDownload}
            aria-label="Unduh log sebagai file"
            className="rounded border border-zinc-700 px-2 py-1 text-xs text-zinc-300 hover:bg-zinc-800"
          >
            Unduh
          </button>
        </div>
      </div>

      <div
        ref={scrollRef}
        onScroll={() => {
          const el = scrollRef.current;
          if (!el) return;
          nearBottomRef.current =
            el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
        className={`flex-1 overflow-y-auto p-3 font-mono text-xs leading-relaxed ${
          wrap ? "" : "overflow-x-auto"
        }`}
      >
        {filteredEntries.length === 0 ? (
          <p className="text-zinc-500">Tidak ada baris yang cocok.</p>
        ) : (
          filteredEntries.map((e) => (
            <div
              key={e.id}
              className={`flex gap-2 ${
                wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre"
              }`}
            >
              <span className="shrink-0 select-none text-zinc-600">
                [{e.stepIndex}]
              </span>
              <span
                className={`min-w-0 break-words ${
                  SOURCE_CLASS[e.source] ?? "text-zinc-300"
                }`}
              >
                {e.message}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
