import { defineConfig, devices } from "@playwright/test";

// Smoke ini menguji preview web Next.js (npm run dev), bukan binary Tauri:
// WebView Tauri tidak bisa di-drive langsung oleh Playwright.
// Next 16 memblokir origin 127.0.0.1 untuk resource dev bila server hanya
// mengizinkan `localhost`; hostname eksplisit membuat origin tersebut valid
// tanpa mengubah next.config.js.
export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:3000",
    headless: true,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: "npm run dev -- --hostname 127.0.0.1",
    url: "http://127.0.0.1:3000",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
