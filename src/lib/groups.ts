import type { Profile } from "@/types";

export interface ProfileGroup {
  name: string | null;
  profiles: Profile[];
}

/**
 * Kelompokkan profile ke folder. Folder yang tersimpan tetap muncul walau
 * profile-nya kosong, supaya user masih bisa rename/hapus/tambah profile di sana.
 * Grup "Tanpa Folder" selalu diletakkan terakhir.
 */
export function groupProfiles(
  profiles: Profile[],
  folders: string[]
): ProfileGroup[] {
  const groupMap = new Map<string | null, Profile[]>();
  const folderSet = new Set(folders);

  for (const p of profiles) {
    const key = p.folder ?? null;
    if (key !== null) folderSet.add(key);
    if (!groupMap.has(key)) groupMap.set(key, []);
    groupMap.get(key)!.push(p);
  }

  const folderNames = [...folderSet].sort((a, b) => a.localeCompare(b));
  const groups: ProfileGroup[] = folderNames.map((name) => ({
    name,
    profiles: (groupMap.get(name) ?? [])
      .slice()
      .sort(
        (a, b) =>
          (a.order ?? 0) - (b.order ?? 0) || a.name.localeCompare(b.name)
      ),
  }));

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

  return groups;
}
