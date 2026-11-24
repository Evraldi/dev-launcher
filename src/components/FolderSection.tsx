"use client";

import type { ReactNode } from "react";

interface Props {
  name: string | null;
  count: number;
  collapsed: boolean;
  isDropTarget: boolean;
  renaming: boolean;
  renameValue: string;
  dropKey: string;
  onToggle: () => void;
  onRenameStart: () => void;
  onRenameChange: (value: string) => void;
  onRenameSubmit: () => void;
  onRenameCancel: () => void;
  onDelete: () => void;
  children: ReactNode;
}

export default function FolderSection({
  name,
  count,
  collapsed,
  isDropTarget,
  renaming,
  renameValue,
  dropKey,
  onToggle,
  onRenameStart,
  onRenameChange,
  onRenameSubmit,
  onRenameCancel,
  onDelete,
  children,
}: Props) {
  const isUngrouped = name === null;

  return (
    <section
      data-drop-folder={dropKey}
      className={`rounded-md border ${
        isDropTarget
          ? "border-emerald-400 bg-emerald-50"
          : "border-zinc-200 bg-zinc-50"
      } transition-colors`}
    >
      <header className="flex items-center gap-1 px-2 py-1.5">
        <button
          onClick={onToggle}
          aria-expanded={!collapsed}
          aria-label={`${collapsed ? "Buka" : "Tutup"} folder ${isUngrouped ? "Tanpa Folder" : name}`}
          className="flex flex-1 items-center gap-1.5 text-left text-xs font-semibold text-zinc-700 hover:text-zinc-900"
        >
          <span className="text-zinc-400">{collapsed ? "▶" : "▼"}</span>
          <span className="truncate">{isUngrouped ? "Tanpa Folder" : name}</span>
          <span className="ml-auto rounded bg-zinc-200 px-1.5 py-0.5 text-[10px] font-medium text-zinc-500">
            {count}
          </span>
        </button>

        {!isUngrouped && !renaming && (
          <div className="flex shrink-0 gap-0.5">
            <button
              onClick={onRenameStart}
              title="Rename folder"
              aria-label={`Rename folder ${name}`}
              className="rounded px-1.5 py-0.5 text-[11px] text-zinc-500 hover:bg-zinc-200 hover:text-zinc-800"
            >
              ✎
            </button>
            <button
              onClick={onDelete}
              title="Hapus folder"
              aria-label={`Hapus folder ${name}`}
              className="rounded px-1.5 py-0.5 text-[11px] text-red-400 hover:bg-red-50 hover:text-red-600"
            >
              ✕
            </button>
          </div>
        )}
      </header>

      {renaming && (
        <form
          className="flex gap-1 px-2 pb-2"
          onSubmit={(e) => {
            e.preventDefault();
            onRenameSubmit();
          }}
        >
          <input
            autoFocus
            value={renameValue}
            onChange={(e) => onRenameChange(e.target.value)}
            placeholder="Nama folder baru"
            className="w-full min-w-0 rounded border border-zinc-300 px-2 py-1 text-xs focus:border-blue-500 focus:outline-none"
          />
          <button
            type="submit"
            className="shrink-0 rounded bg-blue-600 px-2 py-1 text-[11px] font-medium text-white hover:bg-blue-700"
          >
            Simpan
          </button>
          <button
            type="button"
            onClick={onRenameCancel}
            className="shrink-0 rounded border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100"
          >
            Batal
          </button>
        </form>
      )}

      {!collapsed && <div className="flex flex-col gap-2 px-2 pb-2">{children}</div>}
    </section>
  );
}
