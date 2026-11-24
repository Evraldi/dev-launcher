"use client";

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import type {
  ConfigStatus,
  LogEntry,
  PortConflict,
  Profile,
  ProfileStatus,
  StatusPayload,
  StepRuntimeState,
  StepStatusPayload,
} from "@/types";
import ProfileForm from "@/components/ProfileForm";
import LogPanel from "@/components/LogPanel";
import StatusDot from "@/components/StatusDot";
import ConflictModal from "@/components/ConflictModal";
import ConfirmDialog from "@/components/ConfirmDialog";
import FolderSection from "@/components/FolderSection";
import ToastHost, { type ToastKind } from "@/components/ToastHost";
import {
  api,
  isTauri,
  nowMs,
  onProfileStatus,
  onProfileLogBatch,
  onProfileStepStatus,
} from "@/lib/tauri";
import { formatDuration, parsePersistedLogLine } from "@/lib/format";
import { groupProfiles } from "@/lib/groups";

function subscribeNoop(): () => void {
  return () => {};
}

let logId = 0;
let toastId = 0;

interface ToastItem {
  id: number;
  message: string;
  kind: ToastKind;
}

type ConfirmState = {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
};

interface RunStats {
  durationMs: number;
  exitCode: number | null;
  status: ProfileStatus;
}

type DropTarget =
  | { kind: "folder"; folder: string | null }
  | { kind: "profile"; id: string; after: boolean }
  | null;

const UNGROUPED_KEY = "__ungrouped__";

export default function Home() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [confirmState, setConfirmState] = useState<ConfirmState | null>(null);
  const [configStatus, setConfigStatus] = useState<ConfigStatus | null>(null);
  const [runStats, setRunStats] = useState<Record<string, RunStats>>({});
  const [editing, setEditing] = useState<Profile | null | "new">(null);
  const [statuses, setStatuses] = useState<Record<string, ProfileStatus>>({});
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [conflicts, setConflicts] = useState<PortConflict[] | null>(null);
  const [pendingStartId, setPendingStartId] = useState<string | null>(null);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [bulkBusy, setBulkBusy] = useState(false);
  const [formDirty, setFormDirty] = useState(false);
  const [profileLogs, setProfileLogs] = useState<Record<string, LogEntry[]>>(
    {}
  );
  const [stepStatuses, setStepStatuses] = useState<
    Record<string, Record<number, StepRuntimeState>>
  >({});
  const [logTab, setLogTab] = useState<number | "all">("all");
  // Manajemen folder
  const [creatingFolder, setCreatingFolder] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  // `undefined` = tidak sedang rename. `null` dipakai untuk grup "Tanpa Folder",
  // jadi tidak boleh jadi nilai awal (kalau iya, form rename selalu muncul).
  const [renamingFolder, setRenamingFolder] = useState<string | null | undefined>(
    undefined
  );
  const [renameValue, setRenameValue] = useState("");
  const [collapsedFolders, setCollapsedFolders] = useState<Record<string, boolean>>({});
  const [search, setSearch] = useState("");
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
  const seededLogsRef = useRef<Set<string>>(new Set());
  const logClearUntilRef = useRef<Record<string, number>>({});
  const startTimesRef = useRef<Record<string, number>>({});
  const toastTimersRef = useRef<Map<number, number>>(new Map());

  useEffect(() => {
    profilesRef.current = profiles;
  }, [profiles]);

  const dismissToast = useCallback((id: number) => {
    const timer = toastTimersRef.current.get(id);
    if (timer !== undefined) window.clearTimeout(timer);
    toastTimersRef.current.delete(id);
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const pushToast = useCallback(
    (message: string, kind: ToastKind = "info") => {
      const id = ++toastId;
      setToasts((prev) => [...prev.slice(-4), { id, message, kind }]);
      const timer = window.setTimeout(() => dismissToast(id), 4500);
      toastTimersRef.current.set(id, timer);
    },
    [dismissToast]
  );

  const showError = useCallback(
    (message: string) => {
      setError(message);
      pushToast(message, "error");
    },
    [pushToast]
  );

  useEffect(
    () => () => {
      for (const timer of toastTimersRef.current.values()) {
        window.clearTimeout(timer);
      }
      toastTimersRef.current.clear();
    },
    []
  );

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
    const [list, folderList, config] = await Promise.all([
      api.listProfiles(),
      api.listFolders(),
      api.configStatus(),
    ]);
    setProfiles(list);
    setFolders(folderList);
    setConfigStatus(config);
  }

  useEffect(() => {
    if (!isTauri()) return; // Preview web: tidak ada profile yang dimuat.
    let cancelled = false;
    (async () => {
      try {
        const [list, folderList, runtime, config] = await Promise.all([
          api.listProfiles(),
          api.listFolders(),
          api.runtimeState(),
          api.configStatus(),
        ]);
        if (!cancelled) {
          setProfiles(list);
          setFolders(folderList);
          setConfigStatus(config);
          // Proses yang masih hidup dari sesi sebelumnya (mis. app di-restart).
          setStatuses((prev) => {
            const next = { ...prev };
            for (const info of runtime) next[info.profileId] = "running";
            return next;
          });
          // Proses lama tidak punya event step status; tampilkan chip "spawned"
          // untuk step-step profil yang masih hidup supaya UI tidak kosong.
          setStepStatuses((prev) => {
            const next = { ...prev };
            for (const info of runtime) {
              const profile = list.find((p) => p.id === info.profileId);
              if (!profile || next[info.profileId]) continue;
              next[info.profileId] = Object.fromEntries(
                profile.steps.map((_, i) => [i, "spawned" as const])
              );
            }
            return next;
          });
          setLoaded(true);
        }
      } catch (err) {
        if (!cancelled) {
          showError(err instanceof Error ? err.message : String(err));
          setLoaded(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [showError]);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onProfileLogBatch((entries) => {
      setProfileLogs((prev) => {
        const next = { ...prev };
        const byProfile = new Map<string, LogEntry[]>();
        for (const payload of entries) {
          // Batch yang sudah antre sebelum `clear_profile_log` boleh masuk
          // setelah file ditutup; jangan biarkan mengisi ulang log yang baru
          // dikosongkan.
          if (nowMs() < (logClearUntilRef.current[payload.profileId] ?? 0)) {
            continue;
          }
          const list = byProfile.get(payload.profileId) ?? [];
          list.push({
            id: ++logId,
            stepIndex: payload.stepIndex,
            source: payload.source,
            message: payload.message,
          });
          byProfile.set(payload.profileId, list);
        }
        for (const [profileId, incoming] of byProfile) {
          const merged = [...(next[profileId] ?? []), ...incoming];
          // Batasi jumlah baris per profile supaya memori tetap ringan.
          next[profileId] =
            merged.length > 2000 ? merged.slice(merged.length - 2000) : merged;
        }
        return next;
      });
    }).then((un) => {
      // Effect bisa dibersihkan sebelum `listen` selesai (Strict Mode/HMR).
      if (cancelled) un();
      else unlisten = un;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onProfileStatus((payload: StatusPayload) => {
      setStatuses((prev) => ({
        ...prev,
        [payload.profileId]: payload.status,
      }));
      // Event "starting" dipakai sebagai tonggak durasi. Ikut menangkap start
      // dari Start All yang tidak lewat handler tombol per profile.
      if (payload.status === "starting") {
        startTimesRef.current[payload.profileId] = nowMs();
        setRunStats((prev) => {
          if (!prev[payload.profileId]) return prev;
          const next = { ...prev };
          delete next[payload.profileId];
          return next;
        });
      }
      if (payload.status === "failed" && payload.message) {
        showError(payload.message);
      }
      if (payload.status === "stopped" || payload.status === "failed") {
        const startedAt = startTimesRef.current[payload.profileId];
        if (startedAt !== undefined) {
          delete startTimesRef.current[payload.profileId];
          const durationMs = Math.max(0, nowMs() - startedAt);
          const exitCode = payload.exitCode ?? null;
          setRunStats((prev) => ({
            ...prev,
            [payload.profileId]: {
              durationMs,
              exitCode,
              status: payload.status,
            },
          }));
        }
        setStepStatuses((prev) => {
          const next = { ...prev };
          delete next[payload.profileId];
          return next;
        });
      }
    }).then((un) => {
      // Effect bisa dibersihkan sebelum `listen` selesai (Strict Mode/HMR).
      if (cancelled) un();
      else unlisten = un;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [showError]);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onProfileStepStatus((payload: StepStatusPayload) => {
      setStepStatuses((prev) => ({
        ...prev,
        [payload.profileId]: {
          ...(prev[payload.profileId] ?? {}),
          [payload.stepIndex]: payload.state,
        },
      }));
    }).then((un) => {
      // Effect bisa dibersihkan sebelum `listen` selesai (Strict Mode/HMR).
      if (cancelled) un();
      else unlisten = un;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  async function handleSave(profile: Profile) {
    await api.saveProfile(profile);
    setFormDirty(false);
    setEditing(null);
    await refreshAll();
    pushToast(`Profile "${profile.name}" tersimpan.`, "success");
  }

  /// Buka form edit, tapi minta konfirmasi kalau ada perubahan yang belum disimpan.
  function beginEdit(next: Profile | "new") {
    if (next !== "new" && next.external) {
      showError("Profile ini dikelola dari conf.jsonc, bukan lewat UI.");
      return;
    }
    if (editing && formDirty) {
      setConfirmState({
        title: "Perubahan belum disimpan",
        message: "Perubahan pada form belum disimpan. Tinggalkan form ini?",
        confirmLabel: "Tinggalkan perubahan",
        danger: true,
        onConfirm: () => {
          setConfirmState(null);
          setFormDirty(false);
          setEditing(next);
        },
      });
      return;
    }
    setFormDirty(false);
    setEditing(next);
  }

  async function handleDelete(id: string) {
    const target = profiles.find((p) => p.id === id);
    if (target?.external) {
      showError("Profile ini dikelola dari conf.jsonc, hapus lewat file tersebut.");
      return;
    }
    setConfirmState({
      title: "Hapus profile?",
      message: "Hapus profile ini? Proses yang berjalan akan dihentikan.",
      confirmLabel: "Hapus profile",
      danger: true,
      onConfirm: () => {
        setConfirmState(null);
        void deleteProfile(id);
      },
    });
  }

  async function deleteProfile(id: string) {
    try {
      await api.deleteProfile(id);
      if (activeProfileId === id) setActiveProfileId(null);
      if (editing !== "new" && editing?.id === id) {
        setFormDirty(false);
        setEditing(null);
      }
      seededLogsRef.current.delete(id);
      setProfileLogs((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      await refreshAll();
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  }

  function setProfileBusy(id: string, value: boolean) {
    setBusy((prev) => {
      if (!value) {
        const next = { ...prev };
        delete next[id];
        return next;
      }
      return { ...prev, [id]: true };
    });
  }

  async function startProfileWithCheck(id: string, afterStop = false) {
    setError(null);
    setProfileBusy(id, true);
    try {
      // Quick restart: pastikan proses lama benar-benar berhenti dulu supaya
      // preflight tidak melihat port milik profile yang sama masih terpakai.
      if (afterStop) await api.stopProfile(id);
      const result = await api.preflightProfile(id);
      if (result.errors.length > 0) {
        showError(result.errors.join(" "));
        return;
      }
      if (result.conflicts.length > 0) {
        setConflicts(result.conflicts);
        setPendingStartId(id);
        return;
      }
      await api.startProfile(id);
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setProfileBusy(id, false);
    }
  }

  async function handleKillAndContinue() {
    if (!pendingStartId) return;
    const id = pendingStartId;
    setProfileBusy(id, true);
    setBulkBusy(true);
    setError(null);
    try {
      let killError: string | null = null;
      if (conflicts) {
        // Kill hanya proses yang masih memegang port tersebut.
        for (const c of conflicts) {
          for (const pid of c.pids) {
            try {
              await api.killPortProcess(pid, c.port);
            } catch (err) {
              killError = err instanceof Error ? err.message : String(err);
            }
          }
        }
      }
      // Kalau ada kill yang gagal, jangan pakai `force`: itu akan menyalakan
      // profile di atas port yang masih dipakai proses lain.
      if (killError) {
        showError(killError);
        return;
      }

      // Proses baru bisa saja mengambil port di antara kill dan start.
      const check = await api.preflightProfile(id);
      if (check.errors.length > 0) {
        showError(check.errors.join(" "));
        return;
      }
      if (check.conflicts.length > 0) {
        setConflicts(check.conflicts);
        showError("Sebagian port masih bentrok. Periksa lagi lalu pilih tindakan.");
        return;
      }
      setConflicts(null);
      setPendingStartId(null);
      await api.startProfile(id);
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setProfileBusy(id, false);
      setBulkBusy(false);
    }
  }

  async function handleUseAltPort(overrides: Record<number, number>) {
    if (!pendingStartId) return;
    const id = pendingStartId;
    setProfileBusy(id, true);
    setBulkBusy(true);
    try {
      setConflicts(null);
      setPendingStartId(null);
      await api.startProfile(id, { portOverrides: overrides });
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setProfileBusy(id, false);
      setBulkBusy(false);
    }
  }

  async function handleSkipExisting() {
    if (!pendingStartId || !conflicts) return;
    const id = pendingStartId;
    const skipSteps = conflicts.map((c) => c.stepIndex);
    setProfileBusy(id, true);
    setBulkBusy(true);
    try {
      setConflicts(null);
      setPendingStartId(null);
      await api.startProfile(id, { skipSteps });
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setProfileBusy(id, false);
      setBulkBusy(false);
    }
  }

  async function handleStop(id: string) {
    setProfileBusy(id, true);
    try {
      await api.stopProfile(id);
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setProfileBusy(id, false);
    }
  }

  async function handleStartAll() {
    setError(null);
    setBulkBusy(true);
    try {
      const result = await api.startAll();
      const failedNames = result.failed.map((f) => f.name).join(", ");
      const alreadyCount = result.alreadyRunning.length;

      if (result.total === 0) {
        pushToast(
          "Belum ada profile. Tambah lewat UI atau isi conf.jsonc.",
          "info"
        );
      } else if (result.failed.length > 0) {
        showError(
          `Start All: ${result.started} berhasil, ${result.failed.length} gagal${
            failedNames ? `: ${failedNames}` : ""
          }. Detail ada di log masing-masing.`
        );
      } else if (result.started > 0) {
        pushToast(
          `Berhasil start ${result.started} profile${
            alreadyCount > 0 ? `, ${alreadyCount} sudah berjalan` : ""
          }.`,
          "success"
        );
      } else if (alreadyCount > 0) {
        pushToast(
          `Semua ${alreadyCount} profile sudah berjalan.`,
          "info"
        );
      } else {
        pushToast("Tidak ada perubahan dari Start All.", "info");
      }
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setBulkBusy(false);
    }
  }

  async function handleStopAll() {
    setBulkBusy(true);
    try {
      await api.stopAll();
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    } finally {
      setBulkBusy(false);
    }
  }

  function selectActive(id: string) {
    setActiveProfileId(id);
    setLogTab("all");
    loadPersistedLogs(id);
  }

  // Muat log yang dipersist dari disk sekali per profile per sesi.
  async function loadPersistedLogs(id: string) {
    if (seededLogsRef.current.has(id)) return;
    seededLogsRef.current.add(id);
    try {
      const lines = await api.readProfileLog(id);
      const entries: LogEntry[] = lines.map((line) => ({
        id: ++logId,
        ...parsePersistedLogLine(line),
      }));
      if (entries.length > 0) {
        setProfileLogs((prev) => {
          // Jangan timpa log live yang masuk saat `readProfileLog` masih
          // berjalan. Gabungkan, lalu buang duplikat beruntun dari race
          // antara tulisan disk dan event batch.
          const existing = prev[id] ?? [];
          const key = (e: LogEntry) =>
            `${e.stepIndex}|${e.source}|${e.message}`;
          const seen = new Set(existing.slice(-500).map(key));
          const unique = entries.filter((e) => {
            const k = key(e);
            if (seen.has(k)) return false;
            seen.add(k);
            return true;
          });
          const merged = [...existing, ...unique];
          return {
            ...prev,
            [id]: merged.length > 2000 ? merged.slice(merged.length - 2000) : merged,
          };
        });
      }
    } catch (err) {
      // Log live tetap berfungsi, tapi beri tahu supaya tidak terlihat sukses.
      showError(
        `Gagal memuat log tersimpan untuk ${id}: ${
          err instanceof Error ? err.message : String(err)
        }`
      );
    }
  }

  async function handleClearLog(id: string) {
    try {
      await api.clearProfileLog(id);
      logClearUntilRef.current[id] = nowMs() + 750;
      seededLogsRef.current.delete(id);
      setProfileLogs((prev) => ({ ...prev, [id]: [] }));
      setLogTab("all");
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
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
      showError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleRenameFolderSubmit(old: string) {
    const name = renameValue.trim();
    if (!name || name === old) {
      setRenamingFolder(undefined);
      return;
    }
    try {
      await api.renameFolder(old, name);
      setRenamingFolder(undefined);
      await refreshAll();
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleDeleteFolder(name: string) {
    setConfirmState({
      title: "Hapus folder?",
      message: `Hapus folder "${name}"? Profil di dalamnya akan dipindah ke "Tanpa Folder".`,
      confirmLabel: "Hapus folder",
      danger: true,
      onConfirm: () => {
        setConfirmState(null);
        void deleteFolder(name);
      },
    });
  }

  async function deleteFolder(name: string) {
    try {
      await api.deleteFolder(name);
      await refreshAll();
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleOpenConfig() {
    try {
      await api.openConfigFile();
      setConfigStatus(await api.configStatus());
      pushToast("Membuka conf.jsonc di editor/default aplikasi.", "success");
    } catch (err) {
      showError(err instanceof Error ? err.message : String(err));
    }
  }

  // ---- Drag & drop (pointer-based) ----

  function onProfilePointerDown(e: ReactPointerEvent, id: string) {
    // Flag drag sebelumnya bisa tertinggal kalau pointerup jatuh di luar card.
    suppressClickRef.current = false;
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
    if (dragged.external) {
      showError("Profile dari conf.jsonc tidak bisa dipindah lewat UI.");
      return;
    }

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
      showError(err instanceof Error ? err.message : String(err));
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

  const groups = groupProfiles(profiles, folders);

  // ---- Pencarian ----

  const query = search.trim().toLowerCase();
  const isSearching = query.length > 0;
  const searchMatches = (p: Profile) =>
    p.name.toLowerCase().includes(query) ||
    p.steps.some((s) => {
      const haystack =
        s.type === "exe" ? s.path : s.type === "command" ? s.cmd : s.url;
      return haystack.toLowerCase().includes(query);
    });
  const searchResults = isSearching
    ? profiles
        .filter(searchMatches)
        .slice()
        .sort(
          (a, b) =>
            (a.folder ?? "").localeCompare(b.folder ?? "") ||
            (a.order ?? 0) - (b.order ?? 0) ||
            a.name.localeCompare(b.name)
        )
    : [];

  /// Alternatif keyboard untuk drag & drop: Alt+Arrow naik/turun di dalam grup.
  function handleCardKeyDown(e: ReactKeyboardEvent, p: Profile) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selectActive(p.id);
      return;
    }
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    if (p.external) {
      e.preventDefault();
      showError("Profile dari conf.jsonc tidak bisa diurutkan lewat UI.");
      return;
    }
    e.preventDefault();
    const group = groups.find((g) => g.name === (p.folder ?? null))?.profiles ?? [];
    const idx = group.findIndex((item) => item.id === p.id);
    const nextIdx = idx + (e.key === "ArrowDown" ? 1 : -1);
    if (idx < 0 || nextIdx < 0 || nextIdx >= group.length) return;
    void insertAfter(p.id, group[nextIdx].id, p.folder ?? null, e.key === "ArrowDown").catch(
      (err) => showError(err instanceof Error ? err.message : String(err))
    );
  }

  function renderProfileCard(p: Profile) {
    const status = statuses[p.id] ?? "stopped";
    const isBusy = !!busy[p.id];
    const stats = runStats[p.id];
    const isProfileTarget =
      dropTarget?.kind === "profile" && dropTarget.id === p.id;
    return (
      <div
        key={p.id}
        data-profile-id={p.id}
        role="button"
        tabIndex={0}
        aria-label={`Profile ${p.name}, status ${status}${
          p.external ? ", dari conf.jsonc" : ""
        }`}
        aria-keyshortcuts={p.external ? undefined : "Alt+ArrowUp Alt+ArrowDown"}
        title={p.external ? "Dikelola dari conf.jsonc" : "Alt+↑/↓ untuk mengurutkan"}
        onPointerDown={(e) => {
          if (!p.external) onProfilePointerDown(e, p.id);
        }}
        onClick={() => handleCardClick(p.id)}
        onKeyDown={(e) => handleCardKeyDown(e, p)}
        className={`relative cursor-pointer select-none rounded-md border p-3 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 ${
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
            <p className="flex items-center gap-1.5 truncate text-sm font-semibold">
              <span className="truncate">{p.name}</span>
              {p.external && (
                <span className="shrink-0 rounded border border-amber-300 bg-amber-50 px-1 text-[10px] font-medium text-amber-700">
                  JSON
                </span>
              )}
            </p>
            <p className="mt-0.5 truncate text-xs text-zinc-500">
              {p.steps.length} step
            </p>
            {stats && (
              <p className="mt-0.5 truncate text-[11px] text-zinc-400">
                {formatDuration(stats.durationMs)} · exit{" "}
                {stats.exitCode === null ? "—" : stats.exitCode} · {stats.status}
              </p>
            )}
          </div>
          <StatusDot status={status} />
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {status === "running" || status === "starting" ? (
            <>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handleStop(p.id);
                }}
                disabled={isBusy}
                className="rounded-md bg-zinc-800 px-3 py-1.5 text-xs font-medium text-white hover:bg-zinc-900 disabled:opacity-50"
              >
                {isBusy ? "Menghentikan…" : "Stop"}
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  void startProfileWithCheck(p.id, true);
                }}
                disabled={isBusy || bulkBusy}
                title="Stop lalu start ulang profile ini"
                className="rounded-md border border-blue-300 px-3 py-1.5 text-xs font-medium text-blue-700 hover:bg-blue-50 disabled:opacity-50"
              >
                Restart
              </button>
            </>
          ) : (
            <button
              onClick={(e) => {
                e.stopPropagation();
                void startProfileWithCheck(p.id);
              }}
              disabled={isBusy || bulkBusy}
              className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-emerald-700 disabled:opacity-50"
            >
              {isBusy ? "Memeriksa…" : "Start"}
            </button>
          )}
          <button
            onClick={(e) => {
              e.stopPropagation();
              beginEdit(p);
            }}
            disabled={isBusy || p.external}
            title={p.external ? "Edit lewat conf.jsonc" : undefined}
            className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs font-medium text-zinc-700 hover:bg-zinc-50 disabled:opacity-50"
          >
            Edit
          </button>
          <button
            onClick={(e) => {
              e.stopPropagation();
              handleDelete(p.id);
            }}
            disabled={isBusy || p.external}
            title={p.external ? "Hapus lewat conf.jsonc" : undefined}
            className="rounded-md border border-red-200 px-3 py-1.5 text-xs font-medium text-red-600 hover:bg-red-50 disabled:opacity-50"
          >
            Hapus
          </button>
        </div>
      </div>
    );
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
            onClick={handleStartAll}
            disabled={bulkBusy}
            className="rounded-md border border-emerald-300 px-3 py-2 text-sm font-medium text-emerald-700 hover:bg-emerald-50 disabled:opacity-50"
            title="Start semua profile yang belum jalan"
          >
            Start All
          </button>
          <button
            onClick={handleStopAll}
            disabled={bulkBusy}
            className="rounded-md border border-zinc-300 px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-50 disabled:opacity-50"
            title="Stop semua proses"
          >
            Stop All
          </button>
          <button
            onClick={() => beginEdit("new")}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
          >
            + Tambah Profile
          </button>
        </div>
      </header>

      {!isTauriEnv && (
        <div className="mx-6 mt-4 rounded-md bg-amber-50 px-4 py-3 text-sm text-amber-800">
          <strong>Mode pratinjau web.</strong> Jalankan lewat{" "}
          <code>npm run tauri dev</code> untuk fungsionalitas penuh (start/stop
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

      {configStatus && (
        <div className="mx-6 mt-3 flex flex-wrap items-center justify-between gap-2 rounded-md border border-zinc-200 bg-white px-4 py-2 text-xs text-zinc-600">
          <div className="min-w-0">
            <span className="font-semibold text-zinc-700">conf.jsonc</span>
            <span className="ml-2 break-all" title={configStatus.path}>
              {configStatus.path}
            </span>
            {configStatus.error ? (
              <span className="ml-2 font-medium text-red-600">
                Error: {configStatus.error}
              </span>
            ) : !configStatus.exists ? (
              <span className="ml-2 text-amber-600">
                Belum ada — tombol di kanan membuat file kosong.
              </span>
            ) : (
              <span className="ml-2 text-emerald-600">Aktif</span>
            )}
          </div>
          <button
            onClick={() => void handleOpenConfig()}
            className="shrink-0 rounded-md border border-zinc-300 px-2 py-1 font-medium text-zinc-700 hover:bg-zinc-50"
          >
            Buka conf.jsonc
          </button>
        </div>
      )}

      {loading ? (
        <div
          className="flex flex-1 items-center justify-center text-sm text-zinc-500"
          role="status"
          aria-live="polite"
        >
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
            <div className="mb-3">
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Cari profile…"
                className="w-full rounded-md border border-zinc-300 px-2 py-1.5 text-sm focus:border-blue-500 focus:outline-none"
              />
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

            {isSearching ? (
              searchResults.length === 0 ? (
                <p className="mt-8 text-center text-sm text-zinc-500">
                  Tidak ada profile yang cocok dengan &ldquo;{search}&rdquo;.
                </p>
              ) : (
                <div className="flex flex-col gap-3">
                  {searchResults.map(renderProfileCard)}
                </div>
              )
            ) : profiles.length === 0 && folders.length === 0 && !editing && !creatingFolder ? (
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
                      renaming={g.name !== null && renamingFolder === g.name}
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
                      onRenameCancel={() => setRenamingFolder(undefined)}
                      onDelete={() => {
                        if (g.name !== null) handleDeleteFolder(g.name);
                      }}
                      dropKey={key}
                    >
                      {g.profiles.map(renderProfileCard)}
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
                key={editing === "new" ? "new" : editing.id}
                initial={editing === "new" ? null : editing}
                onSave={handleSave}
                onDirtyChange={setFormDirty}
                onCancel={() => {
                  setFormDirty(false);
                  setEditing(null);
                }}
              />
            ) : activeProfileId ? (
              <>
                <div className="mb-2 flex items-center justify-between">
                  <h2 className="text-sm font-semibold">
                    {profiles.find((p) => p.id === activeProfileId)?.name ??
                      "Profile"}
                  </h2>
                  <div className="flex items-center gap-2">
                    <StatusDot
                      status={statuses[activeProfileId] ?? "stopped"}
                    />
                    <button
                      onClick={() => handleClearLog(activeProfileId)}
                      className="rounded-md border border-zinc-300 px-2 py-1 text-xs font-medium text-zinc-600 hover:bg-zinc-50"
                      title="Hapus log yang disimpan di disk"
                    >
                      Bersihkan Log
                    </button>
                  </div>
                </div>
                {(() => {
                  const activeProfile = profiles.find(
                    (p) => p.id === activeProfileId
                  );
                  if (!activeProfile || activeProfile.steps.length === 0)
                    return null;
                  return (
                    <div className="mb-2 flex flex-wrap gap-1.5">
                      {activeProfile.steps.map((step, idx) => {
                        const state = stepStatuses[activeProfileId]?.[idx];
                        const port =
                          step.type === "command" ? step.port : null;
                        const label =
                          step.type === "exe"
                            ? step.path
                            : step.type === "command"
                            ? step.cmd
                            : step.url;
                        const chip =
                          state === "ready"
                            ? "bg-emerald-100 text-emerald-700 border-emerald-300"
                            : state === "spawned"
                            ? "bg-blue-100 text-blue-700 border-blue-300"
                            : state === "timeout"
                            ? "bg-amber-100 text-amber-700 border-amber-300"
                            : state === "skipped"
                            ? "bg-orange-100 text-orange-700 border-orange-300"
                            : "bg-zinc-100 text-zinc-500 border-zinc-200";
                        const kindLabel =
                          step.type === "exe"
                            ? "exe"
                            : step.type === "command"
                            ? "cmd"
                            : "url";
                        return (
                          <button
                            key={idx}
                            onClick={() => setLogTab(idx)}
                            title={label}
                            className={`max-w-56 truncate rounded-md border px-2 py-1 text-xs font-medium transition-colors ${chip}`}
                          >
                            {idx} · {kindLabel}
                            {port ? ` :${port}` : ""}
                          </button>
                        );
                      })}
                    </div>
                  );
                })()}
                <div className="min-h-0 flex-1">
                  <LogPanel
                    entries={profileLogs[activeProfileId] || []}
                    activeStepIndex={logTab}
                    onStepChange={setLogTab}
                  />
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
          busy={bulkBusy}
          onKillAndContinue={handleKillAndContinue}
          onUseAltPort={handleUseAltPort}
          onSkipExisting={handleSkipExisting}
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

      <ToastHost toasts={toasts} onDismiss={dismissToast} />
      {confirmState && (
        <ConfirmDialog
          title={confirmState.title}
          message={confirmState.message}
          confirmLabel={confirmState.confirmLabel}
          danger={confirmState.danger}
          onConfirm={confirmState.onConfirm}
          onCancel={() => setConfirmState(null)}
        />
      )}
    </main>
  );
}
