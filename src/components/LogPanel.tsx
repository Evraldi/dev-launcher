import { useEffect, useRef, useState } from "react";
import type { LogPayload } from "@/types";
import { onProfileLog } from "@/lib/tauri";

interface LogEntry {
  id: number;
  stepIndex: number;
  source: string;
  message: string;
}

let logId = 0;

export default function LogPanel({ profileId }: { profileId: string }) {
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let mounted = true;

    onProfileLog((payload: LogPayload) => {
      if (!mounted || payload.profileId !== profileId) return;
      setEntries((prev) => {
        const next = [...prev];
        // Batasi jumlah baris supaya panel tetap ringan.
        if (next.length > 2000) next.splice(0, next.length - 2000);
        next.push({
          id: ++logId,
          stepIndex: payload.stepIndex,
          source: payload.source,
          message: payload.message,
        });
        return next;
      });
    }).then((un) => {
      if (mounted) unlisten = un;
      else un(); // Jika sudah unmount sebelum listener terpasang, langsung cleanup.
    });

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [profileId]);

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
    <div className="h-full overflow-y-auto rounded-md bg-zinc-950 p-3 font-mono text-xs leading-relaxed">
      {entries.map((e) => (
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
  );
}
