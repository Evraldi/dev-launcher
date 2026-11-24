import { describe, expect, it } from "vitest";
import { groupProfiles } from "../../src/lib/groups";
import type { Profile } from "../../src/types";

function profile(id: string, folder: string | null, order = 0): Profile {
  return {
    id,
    name: id,
    folder,
    order,
    steps: [],
  } as Profile;
}

describe("groupProfiles", () => {
  it("mengembalikan grup kosong kalau tidak ada folder maupun profile", () => {
    expect(groupProfiles([], [])).toEqual([]);
  });

  it("tetap menampilkan folder walau semua profile dihapus", () => {
    expect(groupProfiles([], ["Kerja", "Pribadi"])).toEqual([
      { name: "Kerja", profiles: [] },
      { name: "Pribadi", profiles: [] },
    ]);
  });

  it("memasukkan folder dari profile walaupun belum ada di daftar folder", () => {
    expect(groupProfiles([profile("a", "Kerja")], [])).toEqual([
      { name: "Kerja", profiles: [profile("a", "Kerja")] },
    ]);
  });

  it("mengurutkan folder, profile per order, dan grup Tanpa Folder di akhir", () => {
    const groups = groupProfiles(
      [
        profile("b", null, 1),
        profile("a", "Z", 2),
        profile("c", null, 0),
        profile("d", "A", 0),
      ],
      ["Z", "A"]
    );

    expect(groups.map((g) => g.name)).toEqual(["A", "Z", null]);
    expect(groups[0].profiles.map((p) => p.id)).toEqual(["d"]);
    expect(groups[1].profiles.map((p) => p.id)).toEqual(["a"]);
    expect(groups[2].profiles.map((p) => p.id)).toEqual(["c", "b"]);
  });
});
