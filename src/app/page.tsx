"use client";

import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type {
  LogEntry,
  LogPayload,
  PortConflict,
  Profile,
  ProfileStatus,
  StatusPayload,
} from "@/types";
import ProfileForm from "@/components/ProfileForm";
import LogPanel from "@/components/LogPanel";
import StatusDot from "@/components/StatusDot";
import ConflictModal from "@/components/ConflictModal";
import FolderSection from "@/components/FolderSection";
import { api, isTauri, onProfileStatus, onProfileLog } from "@/lib/tauri";

function subscribeNoop(): () => void {
  return () => {};
}

let logId = 0;

type DropTarget =
  | { kind: "folder"; folder: string | null }
  | { kind: "profile"; id: string; after: boolean }
  | null;

const UNGROUPED_KEY = "__ungrouped__";

export default function Home() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Profile | null | "new">(null);
  const [statuses, setStatuses] = useState<Record<string, ProfileStatus>>({});
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [conflicts, setConflicts] = useState<PortConflict[] | null>(null);
  const [pendingStartId, setPendingStartId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [profileLogs, setProfileLogs] = useState<Record<string, LogEntry[]>>(
    {}
  );
  // Manajemen folder
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [renamingFolder, setRenamingFolder] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [collapsedFolders, setCollapsedFolders] = useState<Record<string, boolean>>({});
  // Drag & drop (pointer-based, lebih andal di WebView daripada HTML5 DnD)
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget>(null);
  const [dragPos, setDragPos] = useState<{ x: number; y: number } | null>(null);
  const dragRef = useRef<{
    id: string;
    startX: number;
    startY: number;
    dragging: boolean;
  } | null>(null);
  const dropTargetRef = useRef<DropTarget>(null);
  const suppressClickRef = useRef(false);
  const profilesRef = useRef(profiles);

  useEffect(() => {
    profilesRef.current = profiles;
  }, [profiles]);
  // Deteksi Tauri via useSyncExternalStore: server (static export) dan render
  // pertama di client sama-sama menghasilkan `false`, lalu berubah setelah
  // hydration tanpa setState sinkron di dalam effect → tidak terjadi
  // hydration mismatch.
  const isTauriEnv = useSyncExternalStore(
    subscribeNoop,
    () => (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window),
    () => false
  );
  // Dimuat hanya saat berada di dalam Tauri; di preview web langsung `true`
  // sehingga list profile kosong ditampilkan (tanpa error API).
  const [loaded, setLoaded] = useState(false);
  const loading = isTauriEnv && !loaded;

  async function refreshAll() {
    const [list, folderList] = await Promise.all([
      api.listProfiles(),
      api.listFolders(),
    ]);
    setProfiles(list);
    setFolders(folderList);
  }

  useEffect(() => {
    if (!isTauri()) return; // Preview web: tidak ada profile yang dimuat.
    let cancelled = false;
    (async () => {
      try {
        const [list, folderList] = await Promise.all([
          api.listProfiles(),
          api.listFolders(),
        ]);
        if (!cancelled) {
          setProfiles(list);
          setFolders(folderList);
          setLoaded(true);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setLoaded(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    onProfileLog((payload: LogPayload) => {
      setProfileLogs((prev) => {
        const current = prev[payload.profileId] || [];
        const next = [...current];
        // Batasi jumlah baris per profile supaya tetap ringan.
        if (next.length > 2000) next.splice(0, next.length - 2000);
        next.push({
          id: ++logId,
          stepIndex: payload.stepIndex,
          source: payload.source,
          message: payload.message,
        });
        return { ...prev, [payload.profileId]: next };
      });
    }).then((un) => (unlisten = un));
    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    onProfileStatus((payload: StatusPayload) => {
      setStatuses((prev) => ({
        ...prev,
        [payload.profileId]: payload.status,
      }));
    }).then((un) => (unlisten = un));
    return () => {
      unlisten?.();
    };
  }, []);

  async function handleSave(profile: Profile) {
    await api.saveProfile(profile);
    setEditing(null);
    await refreshAll();
  }

  async function handleDelete(id: string) {
    if (!confirm("Hapus profile ini?")) return;
    try {
      await api.deleteProfile(id);
      await refreshAll();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function startProfileWithCheck(id: string) {
    setError(null);
    setStarting(true);
    try {
      const found = await api.checkPortConflicts(id);
      if (found.length > 0) {
        setConflicts(found);
        setPendingStartId(id);
        setStarting(false);
        return;
      }
      await api.startProfile(id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setStarting(false);
    }
  }

  async function handleKillAndContinue() {
    if (!pendingStartId) return;
    setStarting(true);
    try {
      if (conflicts) {
        for (const c of conflicts) {
          for (const pid of c.pids) {
            await api.killProcess(pid);
          }
        }
      }
      setConflicts(null);
      await api.startProfile(pendingStartId);
      setPendingStartId(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setStarting(false);
    }
  }

  async function handleUseAltPort() {
    // Catatan: auto-ganti port tidak bisa untuk semua project.
    // Untuk sekarang kita hanya memberi tahu user bahwa fitur ini
    // hanya berlaku untuk project yang membaca env PORT/APP_PORT.
    if (!pendingStartId) return;
    setConflicts(null);
    setPendingStartId(null);
    setError(
      "Ganti port otomatis hanya didukung untuk project yang membaca env PORT/APP_PORT. Silakan set port alternatif di form profile, atau kill proses yang bentrok."
    );
  }

  async function handleStop(id: string) {
    try {
      await api.stopProfile(id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  function selectActive(id: string) {
    setActiveProfileId(id);
  }

  // ---- Manajemen folder ----

  async function handleCreateFolder(e: React.FormEvent) {
    e.preventDefault();
    const name = newFolderName.trim();
    if (!name) return;
    try {
      await api.createFolder(name);
      setNewFolderName("");
      setCreatingFolder(false);
      await refreshAll();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleRenameFolderSubmit(old: string) {
    const name = renameValue.trim();
    if (!name || name === old) {
      setRenamingFolder(null);
      return;
    }
    try {
      await api.renameFolder(old, name);
      setRenamingFolder(null);
      await refreshAll();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleDeleteFolder(name: string) {
    if (
      !confirm(
        `Hapus folder "${name}"? Profil di dalamnya akan dipindah ke "Tanpa Folder".`
      )
    )
      return;
    try {
      await api.deleteFolder(name);
      await refreshAll();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  // ---- Drag & drop (pointer-based) ----

  function onProfilePointerDown(e: ReactPointerEvent, id: string) {
    if (e.button !== 0) return;
    // Jangan mulai drag dari tombol (Start/Stop/Edit/Hapus).
    if ((e.target as HTMLElement).closest("button")) return;
    dragRef.current = {
      id,
      startX: e.clientX,
      startY: e.clientY,
      dragging: false,
    };
  }

  function handleCardClick(id: string) {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    selectActive(id);
  }

  async function insertAfter(
    draggedId: string,
    targetId: string,
    folder: string | null,
    after: boolean
  ) {
    const list = profilesRef.current;
    const groupIds = list
      .filter((p) => (p.folder ?? null) === folder)
      .map((p) => p.id);
    const from = groupIds.indexOf(draggedId);
    if (from >= 0) groupIds.splice(from, 1);
    let to = groupIds.indexOf(targetId);
    if (to < 0) to = groupIds.length;
    groupIds.splice(to + (after ? 1 : 0), 0, draggedId);
    await api.reorderProfiles(groupIds);
  }

  async function performDrop(draggedId: string, target: DropTarget) {
    if (!target) return;
    const list = profilesRef.current;
    const dragged = list.find((p) => p.id === draggedId);
    if (!dragged) return;

    try {
      if (target.kind === "folder") {
        await api.moveProfile(draggedId, target.folder);
      } else {
        const targetProfile = list.find((p) => p.id === target.id);
        if (!targetProfile) return;
        const targetFolder = targetProfile.folder ?? null;
        const draggedFolder = dragged.folder ?? null;
        if (draggedFolder !== targetFolder) {
          await api.moveProfile(draggedId, targetFolder);
        }
        await insertAfter(draggedId, targetProfile.id, targetFolder, target.after);
      }
      await refreshAll();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  useEffect(() => {
    function handleMove(e: PointerEvent) {
      const d = dragRef.current;
      if (!d) return;

      if (!d.dragging) {
        const moved = Math.hypot(e.clientX - d.startX, e.clientY - d.startY);
        if (moved < 6) return; // masih kemungkinan klik biasa
        d.dragging = true;
        setDraggingId(d.id);
      }

      setDragPos({ x: e.clientX, y: e.clientY });

      // Cari target drop berdasarkan elemen di bawah kursor.
      const el = document.elementFromPoint(
        e.clientX,
        e.clientY
      ) as HTMLElement | null;
      let next: DropTarget = null;
      if (el) {
        const cardEl = el.closest("[data-profile-id]") as HTMLElement | null;
        if (cardEl) {
          const targetId = cardEl.getAttribute("data-profile-id")!;
          if (targetId !== d.id) {
            const rect = cardEl.getBoundingClientRect();
            const after = e.clientY > rect.top + rect.height / 2;
            next = { kind: "profile", id: targetId, after };
          }
        } else {
          const folderEl = el.closest(
            "[data-drop-folder]"
          ) as HTMLElement | null;
          if (folderEl) {
            const attr = folderEl.getAttribute("data-drop-folder")!;
            next = {
              kind: "folder",
              folder: attr === UNGROUPED_KEY ? null : attr,
            };
          }
        }
      }
      dropTargetRef.current = next;
      setDropTarget(next);
    }

    function endDrag() {
      const d = dragRef.current;
      if (!d) return;
      dragRef.current = null;
      if (d.dragging) {
        suppressClickRef.current = true;
        performDrop(d.id, dropTargetRef.current);
      }
      dropTargetRef.current = null;
      setDraggingId(null);
      setDropTarget(null);
      setDragPos(null);
    }

    function handleUp() {
      endDrag();
    }

    function handleCancel() {
      dragRef.current = null;
      dropTargetRef.current = null;
      setDraggingId(null);
      setDropTarget(null);
      setDragPos(null);
    }

    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
    window.addEventListener("pointercancel", handleCancel);
    return () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
      window.removeEventListener("pointercancel", handleCancel);
    };
    // Listener didaftarkan sekali; fungsi di dalamnya memakai ref, bukan state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---- Tampilan grup ----

  const groupMap = new Map<string | null, Profile[]>();
  const folderSet = new Set(folders);
  for (const p of profiles) {
    const key = p.folder ?? null;
    if (key !== null) folderSet.add(key);
    if (!groupMap.has(key)) groupMap.set(key, []);
    groupMap.get(key)!.push(p);
  }
  const folderNames = [...folderSet].sort((a, b) => a.localeCompare(b));
  const groups: { name: string | null; profiles: Profile[] }[] = folderNames.map(
    (name) => ({
      name,
      profiles: (groupMap.get(name) ?? [])
        .slice()
        .sort(
          (a, b) =>
            (a.order ?? 0) - (b.order ?? 0) || a.name.localeCompare(b.name)
        ),
    })
  );
  if (groupMap.has(null)) {
    groups.push({
      name: null,
      profiles: (groupMap.get(null) ?? [])
        .slice()
        .sort(
          (a, b) =>
            (a.order ?? 0) - (b.order ?? 0) || a.name.localeCompare(b.name)
        ),
    });
  }

  return (
    <main className="flex h-screen flex-col overflow-hidden bg-zinc-50 text-zinc-900">
      <header className="flex items-center justify-between border-b border-zinc-200 bg-white px-6 py-4">
        <div>
          <h1 className="text-lg font-semibold">Dev Launcher</h1>
          <p className="text-xs text-zinc-500">
            One-click starter untuk semua proses project
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setEditing("new")}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
          >
            + Tambah Profile
          </button>
        </div>
      </header>

      {!isTauriEnv && (
        <div className="mx-6 mt-4 rounded-md bg-amber-50 px-4 py-3 text-sm text-amber-800">
          <strong>Mode pratinjau web.</strong> Jalankan lewat{" "}
          <code>cargo tauri dev</code> untuk fungsionalitas penuh (start/stop
          proses, log, deteksi port, dan folder).
        </div>
      )}

      {error && (
        <div className="mx-6 mt-4 flex items-center justify-between rounded-md bg-red-50 px-4 py-3 text-sm text-red-700">
          <span>{error}</span>
          <button
            onClick={() => setError(null)}
            className="font-semibold text-red-500 hover:text-red-700"
          >
            ✕
          </button>
        </div>
      )}

      {loading ? (
        <div className="flex flex-1 items-center justify-center text-sm text-zinc-500">
          Memuat profile…
        </div>
      ) : (
        <div className="flex flex-1 overflow-hidden">
          {/* Daftar profile */}
          <aside className="w-80 shrink-0 overflow-y-auto border-r border-zinc-200 bg-white p-4">
            <div className="mb-3 flex items-center justify-between">
              <span className="text-xs font-semibold uppercase tracking-wide text-zinc-400">
                Profiles
              </span>
              <button
                onClick={() => setCreatingFolder(true)}
                className="rounded-md border border-zinc-300 px-2 py-1 text-xs font-medium text-zinc-700 hover:bg-zinc-50"
              >
                + Folder
              </button>
            </div>
            {creatingFolder && (
              <form
                onSubmit={handleCreateFolder}
                className="mb-3 flex gap-2"
              >
                <input
                  autoFocus
                  value={newFolderName}
                  onChange={(e) => setNewFolderName(e.target.value)}
                  placeholder="Nama folder baru"
                  className="w-full min-w-0 rounded-md border border-zinc-300 px-2 py-1.5 text-sm focus:border-blue-500 focus:outline-none"
                />
                <button
                  type="submit"
                  className="shrink-0 rounded-md bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-700"
                >
                  Tambah
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setCreatingFolder(false);
                    setNewFolderName("");
                  }}
                  className="shrink-0 rounded-md border border-zinc-300 px-3 py-1.5 text-xs text-zinc-600 hover:bg-zinc-50"
                >
                  Batal
                </button>
              </form>
            )}

            {profiles.length === 0 && !editing && !creatingFolder ? (
              <p className="mt-8 text-center text-sm text-zinc-500">
                Belum ada profile. Klik &ldquo;+ Tambah Profile&rdquo; untuk
                memulai.
              </p>
            ) : (
              <div className="flex flex-col gap-3">
                {groups.map((g) => {
                  const key = g.name ?? UNGROUPED_KEY;
                  const collapsed = !!collapsedFolders[key];
                  return (
                    <FolderSection
                      key={key}
                      name={g.name}
                      count={g.profiles.length}
                      collapsed={collapsed}
                      isDropTarget={
                        dropTarget?.kind === "folder" &&
                        dropTarget.folder === g.name
                      }
                      renaming={renamingFolder === g.name}
                      renameValue={renameValue}
                      onToggle={() =>
                        setCollapsedFolders((prev) => ({
                          ...prev,
                          [key]: !collapsed,
                        }))
                      }
                      onRenameStart={() => {
                        setRenamingFolder(g.name);
                        setRenameValue(g.name ?? "");
                      }}
                      onRenameChange={setRenameValue}
                      onRenameSubmit={() => {
                        if (g.name !== null) handleRenameFolderSubmit(g.name);
                      }}
                      onRenameCancel={() => setRenamingFolder(null)}
                      onDelete={() => {
                        if (g.name !== null) handleDeleteFolder(g.name);
                      }}
                      dropKey={key}
                    >
                      {g.profiles.map((p) => {
                        const status = statuses[p.id] ?? "stopped";
                        const isProfileTarget =
                          dropTarget?.kind === "profile" &&
                          dropTarget.id === p.id;
                        return (
                          <div
                            key={p.id}
                            data-profile-id={p.id}
                            onPointerDown={(e) => onProfilePointerDown(e, p.id)}
                            onClick={() => handleCardClick(p.id)}
                            className={`relative cursor-pointer select-none rounded-md border p-3 transition-colors ${
                              activeProfileId === p.id
                                ? "border-blue-500 bg-blue-50"
                                : "border-zinc-200 bg-white hover:border-zinc-300"
                            } ${draggingId === p.id ? "opacity-40" : ""}`}
                          >
                            {isProfileTarget && !dropTarget!.after && (
                              <span className="absolute -top-1 left-2 right-2 h-0.5 rounded bg-emerald-500" />
                            )}
                            {isProfileTarget && dropTarget!.after && (
                              <span className="absolute -bottom-1 left-2 right-2 h-0.5 rounded bg-emerald-500" />
                            )}
                            <div className="flex items-start justify-between gap-2">
                              <div className="min-w-0">
                                <p className="truncate text-sm font-semibold">
                                  {p.name}
                                </p>
                                <p className="mt-0.5 truncate text-xs text-zinc-500">
                                  {p.steps.length} step
                                </p>
                              </div>
                              <StatusDot status={status} />
                            </div>
                            <div className="mt-3 flex items-center gap-2">
                              {status === "running" || status === "starting" ? (
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    handleStop(p.id);
                                  }}
                                  className="rounded-md bg-zinc-800 px-3 py-1.5 text-xs font-medium text-white hover:bg-zinc-900"
                                >
                                  Stop
                                </button>
                              ) : (
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    startProfileWithCheck(p.id);
                                  }}
                                  disabled={starting}
                                  className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-emerald-700 disabled:opacity-50"
                                >
                                  Start
                                </button>
                              )}
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  setEditing(p);
                                }}
                                className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 hover:bg-zinc-50"
                              >
                                Edit
                              </button>
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleDelete(p.id);
                                }}
                                className="rounded-md border border-red-200 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50"
                              >
                                Hapus
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </FolderSection>
                  );
                })}
              </div>
            )}
          </aside>

          {/* Konten kanan */}
          <section
            className={`min-h-0 flex-1 flex-col p-4 ${
              editing ? "flex overflow-y-auto" : "flex overflow-hidden"
            }`}
          >
            {editing ? (
              <ProfileForm
                initial={editing === "new" ? null : editing}
                onSave={handleSave}
                onCancel={() => setEditing(null)}
              />
            ) : activeProfileId ? (
              <>
                <div className="mb-2 flex items-center justify-between">
                  <h2 className="text-sm font-semibold">
                    {profiles.find((p) => p.id === activeProfileId)?.name ??
                      "Profile"}
                  </h2>
                  <StatusDot status={statuses[activeProfileId] ?? "stopped"} />
                </div>
                <div className="min-h-0 flex-1">
                  <LogPanel entries={profileLogs[activeProfileId] || []} />
                </div>
              </>
            ) : (
              <div className="flex flex-1 items-center justify-center">
                <p className="text-sm text-zinc-400">
                  Pilih profile di kiri untuk melihat log, atau buat profile
                  baru.
                </p>
              </div>
            )}
          </section>
        </div>
      )}

      {conflicts && pendingStartId && (
        <ConflictModal
          conflicts={conflicts}
          busy={starting}
          onKillAndContinue={handleKillAndContinue}
          onUseAltPort={handleUseAltPort}
          onCancel={() => {
            setConflicts(null);
            setPendingStartId(null);
          }}
        />
      )}

      {draggingId && dragPos && (
        <div
          className="pointer-events-none fixed z-50 rounded-md border border-zinc-300 bg-white px-2 py-1 text-xs font-medium text-zinc-700 shadow-md"
          style={{ left: dragPos.x + 12, top: dragPos.y + 12 }}
        >
          {profiles.find((p) => p.id === draggingId)?.name ?? "Profile"}
        </div>
      )}
    </main>
  );
}
