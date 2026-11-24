/** Hasil parsing satu baris log yang dipersist ke disk. */
export interface PersistedLogLine {
  stepIndex: number;
  source: "stdout" | "stderr" | "system";
  message: string;
}

/**
 * Parse baris log tersimpan dengan format:
 * `[timestamp] [step N] [stdout|stderr|system] pesan`
 *
 * Baris yang tidak cocok dianggap pesan system untuk step 0.
 */
export function parsePersistedLogLine(line: string): PersistedLogLine {
  const m = line.match(/^\[[^\]]*\] \[step (\d+)\] \[([^\]]*)\] (.*)$/);
  if (!m) return { stepIndex: 0, source: "system", message: line };
  const src = m[2];
  return {
    stepIndex: Number(m[1]),
    source: src === "stdout" || src === "stderr" ? src : "system",
    message: m[3],
  };
}

/** Format durasi milidetik menjadi `123 ms`, `5s`, atau `1m 5s`. */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const totalSec = Math.floor(ms / 1000);
  const min = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  return min > 0 ? `${min}m ${sec}s` : `${sec}s`;
}
