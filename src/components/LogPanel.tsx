import { useEffect, useRef, useState } from "react";
import type { LogEntry } from "@/types";

interface Props {
  entries: LogEntry[];
  activeStepIndex?: number | "all";
  onStepChange?: (step: number | "all") => void;
}

export default function LogPanel({ entries, activeStepIndex, onStepChange }: Props) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const [internalStep, setInternalStep] = useState<number | "all">("all");

  const isControlled = activeStepIndex !== undefined;
  const activeStep = isControlled ? activeStepIndex : internalStep;
  const setStep = isControlled
    ? (onStepChange ?? (() => {}))
    : setInternalStep;

  // Group entries by step untuk tab navigation
  const stepIndices = Array.from(
    new Set(entries.map((e) => e.stepIndex))
  ).sort((a, b) => a - b);
  const filteredEntries =
    activeStep === "all"
      ? entries
      : entries.filter((e) => e.stepIndex === activeStep);

  // Auto-scroll ke bawah saat ada log baru
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [entries]);

  if (entries.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-zinc-500">
        Belum ada log. Tekan Start untuk menjalankan profile.
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Tab buttons untuk switch antar step */}
      {stepIndices.length > 1 && (
        <div className="flex gap-1 border-b border-zinc-700 bg-zinc-900 p-2">
          <button
            onClick={() => setStep("all")}
            className={`rounded px-3 py-1 text-xs font-medium transition-colors ${
              activeStep === "all"
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
              className={`rounded px-3 py-1 text-xs font-medium transition-colors ${
                activeStep === idx
                  ? "bg-zinc-700 text-white"
                  : "text-zinc-400 hover:bg-zinc-800 hover:text-zinc-300"
              }`}
            >
              Step {idx}
            </button>
          ))}
        </div>
      )}

      {/* Log content */}
      <div className="flex-1 overflow-y-auto rounded-md bg-zinc-950 p-3 font-mono text-xs leading-relaxed">
        {filteredEntries.map((e) => (
        <div key={e.id} className="flex gap-2">
          <span className="shrink-0 select-none text-zinc-600">
            [{e.stepIndex}]
          </span>
          <span
            className={
              e.source === "stderr"
                ? "text-red-400"
                : e.source === "system"
                ? "text-sky-400"
                : "text-zinc-300"
            }
          >
            {e.message}
          </span>
        </div>
      ))}
      <div ref={bottomRef} />
      </div>
    </div>
  );
}
