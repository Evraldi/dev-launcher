import { describe, expect, it } from "vitest";
import { formatDuration, parsePersistedLogLine } from "@/lib/format";

describe("formatDuration", () => {
  it.each([
    [0, "0 ms"],
    [1, "1 ms"],
    [999, "999 ms"],
    [1000, "1s"],
    [5000, "5s"],
    [65_000, "1m 5s"],
    [120_000, "2m 0s"],
  ])("memformat %i ms sebagai %s", (ms, expected) => {
    expect(formatDuration(ms)).toBe(expected);
  });
});

describe("parsePersistedLogLine", () => {
  it("mengurai baris stdout yang valid", () => {
    expect(
      parsePersistedLogLine(
        "[2026-01-01T00:00:00.000Z] [step 3] [stdout] halo dunia"
      )
    ).toEqual({ stepIndex: 3, source: "stdout", message: "halo dunia" });
  });

  it("mengurai baris stderr yang valid", () => {
    expect(
      parsePersistedLogLine("[t] [step 12] [stderr] gagal startup")
    ).toEqual({ stepIndex: 12, source: "stderr", message: "gagal startup" });
  });

  it("mengurai baris system yang valid", () => {
    expect(
      parsePersistedLogLine("[t] [step 0] [system] proses dimulai")
    ).toEqual({ stepIndex: 0, source: "system", message: "proses dimulai" });
  });

  it("mengubah sumber yang tidak dikenal menjadi system", () => {
    expect(parsePersistedLogLine("[t] [step 2] [debug] pesan")).toEqual({
      stepIndex: 2,
      source: "system",
      message: "pesan",
    });
  });

  it("mempertahankan pesan kosong pada baris yang valid", () => {
    expect(parsePersistedLogLine("[t] [step 1] [stdout] ")).toEqual({
      stepIndex: 1,
      source: "stdout",
      message: "",
    });
  });

  it("memakai fallback untuk baris yang tidak cocok", () => {
    expect(parsePersistedLogLine("log polos tanpa format")).toEqual({
      stepIndex: 0,
      source: "system",
      message: "log polos tanpa format",
    });
    expect(parsePersistedLogLine("")).toEqual({
      stepIndex: 0,
      source: "system",
      message: "",
    });
  });
});
