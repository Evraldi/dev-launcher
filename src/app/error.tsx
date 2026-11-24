"use client";

import { useEffect } from "react";

interface Props {
  error: Error & { digest?: string };
  unstable_retry: () => void;
}

export default function Error({ error, unstable_retry }: Props) {
  useEffect(() => {
    console.error(error);
  }, [error]);

  return (
    <main
      role="alert"
      className="flex min-h-[60vh] flex-1 items-center justify-center bg-zinc-50 p-6"
    >
      <div className="w-full max-w-md rounded-lg border border-zinc-200 bg-white p-6 shadow-sm">
        <h1 className="mb-2 text-lg font-semibold text-zinc-900">
          Terjadi kesalahan
        </h1>
        <p className="mb-4 text-sm leading-relaxed text-zinc-600">
          Halaman gagal dimuat. Coba lagi, atau muat ulang aplikasi jika
          masalahnya berlanjut.
        </p>
        {error.digest && (
          <p className="mb-4 break-all font-mono text-xs text-zinc-400">
            Kode error: {error.digest}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => unstable_retry()}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus-visible:ring-2 focus-visible:ring-blue-400"
          >
            Coba lagi
          </button>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="rounded-md border border-zinc-300 px-4 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50 focus-visible:ring-2 focus-visible:ring-zinc-400"
          >
            Muat ulang
          </button>
        </div>
      </div>
    </main>
  );
}
