import type { ProfileStatus } from "@/types";

const colors: Record<ProfileStatus, string> = {
  running: "bg-emerald-500",
  starting: "bg-amber-400 animate-pulse",
  stopped: "bg-zinc-400",
  failed: "bg-red-500",
};

const labels: Record<ProfileStatus, string> = {
  running: "Berjalan",
  starting: "Memulai…",
  stopped: "Berhenti",
  failed: "Gagal",
};

export default function StatusDot({ status }: { status: ProfileStatus }) {
  return (
    <span
      className="inline-flex items-center gap-2"
      role="status"
      aria-label={`Status: ${labels[status]}`}
    >
      <span
        aria-hidden="true"
        className={`inline-block h-2.5 w-2.5 rounded-full ${colors[status]}`}
      />
      <span className="text-xs text-zinc-500">{labels[status]}</span>
    </span>
  );
}
