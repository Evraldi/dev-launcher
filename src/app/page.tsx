"use client";

import { useEffect, useState } from "react";
import type { PortConflict, Profile, ProfileStatus, StatusPayload } from "@/types";
import ProfileForm from "@/components/ProfileForm";
import LogPanel from "@/components/LogPanel";
import StatusDot from "@/components/StatusDot";
import ConflictModal from "@/components/ConflictModal";
import { api, isTauri, onProfileStatus } from "@/lib/tauri";

export default function Home() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Profile | null | "new">(null);
  const [statuses, setStatuses] = useState<Record<string, ProfileStatus>>({});
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [conflicts, setConflicts] = useState<PortConflict[] | null>(null);
  const [pendingStartId, setPendingStartId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  async function refreshProfiles() {
    const list = await api.listProfiles();
    setProfiles(list);
  }

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await api.listProfiles();
        if (!cancelled) {
          setProfiles(list);
          setLoading(false);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setLoading(false);
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
    await refreshProfiles();
  }

  async function handleDelete(id: string) {
    if (!confirm("Hapus profile ini?")) return;
    try {
      await api.deleteProfile(id);
      await refreshProfiles();
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

  return (
    <main className="flex h-screen flex-col overflow-hidden bg-zinc-50 text-zinc-900">
      <header className="flex items-center justify-between border-b border-zinc-200 bg-white px-6 py-4">
        <div>
          <h1 className="text-lg font-semibold">Dev Launcher</h1>
          <p className="text-xs text-zinc-500">
            One-click starter untuk semua proses project
          </p>
        </div>
        <button
          onClick={() => setEditing("new")}
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
        >
          + Tambah Profile
        </button>
      </header>

      {!isTauri() && (
        <div className="mx-6 mt-4 rounded-md bg-amber-50 px-4 py-3 text-sm text-amber-800">
          <strong>Mode pratinjau web.</strong> Jalankan lewat{" "}
          <code>cargo tauri dev</code> untuk fungsionalitas penuh (start/stop
          proses, log, dan deteksi port).
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
            {profiles.length === 0 && !editing ? (
              <p className="mt-8 text-center text-sm text-zinc-500">
                Belum ada profile. Klik &ldquo;+ Tambah Profile&rdquo; untuk
                memulai.
              </p>
            ) : (
              <div className="flex flex-col gap-3">
                {profiles.map((p) => {
                  const status = statuses[p.id] ?? "stopped";
                  return (
                    <div
                      key={p.id}
                      onClick={() => selectActive(p.id)}
                      className={`cursor-pointer rounded-md border p-3 transition-colors ${
                        activeProfileId === p.id
                          ? "border-blue-500 bg-blue-50"
                          : "border-zinc-200 bg-white hover:border-zinc-300"
                      }`}
                    >
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
                  <LogPanel key={activeProfileId} profileId={activeProfileId} />
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
    </main>
  );
}
