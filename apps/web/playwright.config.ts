// Tests de bout en bout : Playwright lance le vrai serveur (`mementum serve`, sur un
// dossier de données temporaire), puis Vite, et les tests passent par le mandataire de
// Vite comme un navigateur. Ports, contrôle des ports pris et dossier de données :
// `e2e/servers.ts`.
//
// Les budgets du navigateur (`e2e/budgets.spec.ts`) n'en font pas partie : ils se mesurent
// sur un build de production et un serveur `--release` (`playwright.budgets.config.ts`,
// Ruling R38).
import { defineConfig } from "@playwright/test";
import { API_PORT, env, prepareServers, REPO, WEB, WEB_PORT } from "./e2e/servers";

const dataDir = prepareServers();

export default defineConfig({
  testDir: "./e2e",
  testIgnore: "budgets.spec.ts",
  timeout: 60_000,
  // Les mesures de latence demandent une machine au calme : un test à la fois.
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${WEB_PORT}`,
    headless: true,
    trace: "off",
  },
  webServer: [
    {
      // La première compilation du serveur peut être longue : délai généreux.
      command: `cargo run -p mementum-cli -- serve --bind 127.0.0.1:${API_PORT} --data-dir "${dataDir}"`,
      cwd: REPO,
      url: `http://127.0.0.1:${API_PORT}/health`,
      reuseExistingServer: false,
      timeout: 900_000,
      stdout: "ignore",
      stderr: "pipe",
      env,
    },
    {
      command: `node node_modules/vite/bin/vite.js --port ${WEB_PORT} --strictPort`,
      cwd: WEB,
      url: `http://localhost:${WEB_PORT}`,
      reuseExistingServer: false,
      timeout: 120_000,
      env,
    },
  ],
});
