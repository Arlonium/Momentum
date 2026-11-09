import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 120_000,
  workers: 1,
  reporter: [["list"]],
  use: { baseURL: "http://localhost:5173", headless: true },
  webServer: [
    {
      command: "cargo run --release -p spike-server --bin spike-server",
      cwd: "..",
      url: "http://localhost:8080/health",
      reuseExistingServer: true,
      timeout: 600_000,
    },
    {
      command: "pnpm dev",
      url: "http://localhost:5173",
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
});
