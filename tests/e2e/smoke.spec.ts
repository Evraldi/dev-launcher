import { expect, test } from "@playwright/test";

// Preview web sengaja dijalankan tanpa __TAURI_INTERNALS__, jadi tidak perlu
// data profile: pastikan shell UI tampil dan tidak ada error runtime.
test("preview web menampilkan shell Dev Launcher tanpa error", async ({
  page,
}) => {
  const runtimeErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") runtimeErrors.push(message.text());
  });
  page.on("pageerror", (error) => runtimeErrors.push(error.message));

  await page.goto("/");

  await expect(
    page.getByRole("heading", { name: "Dev Launcher", level: 1 })
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "+ Tambah Profile" })
  ).toBeVisible();
  await expect(page.getByText("Mode pratinjau web.", { exact: false })).toBeVisible();

  expect(runtimeErrors).toEqual([]);
});
