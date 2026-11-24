"use client";

export type ToastKind = "success" | "error" | "info";

export interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
}

interface Props {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}

const TOAST_CLASS: Record<ToastKind, string> = {
  success: "border-emerald-200 bg-emerald-50 text-emerald-800",
  error: "border-red-200 bg-red-50 text-red-800",
  info: "border-zinc-200 bg-white text-zinc-800",
};

export default function ToastHost({ toasts, onDismiss }: Props) {
  return (
    <div className="pointer-events-none fixed right-4 top-4 z-[60] flex w-[min(24rem,calc(100vw-2rem))] flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role={toast.kind === "error" ? "alert" : "status"}
          aria-live={toast.kind === "error" ? undefined : "polite"}
          className={`pointer-events-auto flex items-start gap-3 rounded-md border px-3 py-2 text-sm shadow-md ${TOAST_CLASS[toast.kind]}`}
        >
          <span className="min-w-0 flex-1 break-words">{toast.message}</span>
          <button
            type="button"
            onClick={() => onDismiss(toast.id)}
            aria-label="Tutup notifikasi"
            className="shrink-0 rounded px-1 text-base leading-none text-zinc-500 hover:bg-black/5 hover:text-zinc-800 focus-visible:outline-2 focus-visible:outline-zinc-500"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
