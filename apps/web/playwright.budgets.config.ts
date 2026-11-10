// Budgets du navigateur (§5.10, `e2e/budgets.spec.ts`), mesurés sur ce que les gens
// utiliseront (Ruling R38) : le serveur compilé en release, et l'application construite
// par `vite build`, servie par `vite preview`, dont le mandataire est celui de Vite en
// développement (`vite.config.ts`). Sur Vite en développement et un serveur debug, un pic
// sur cinq ouvertures faisait échouer la suite environ une fois sur sept.
//
// `corepack pnpm -C apps/web e2e:budgets`. Ports, contrôle des ports pris, dossier de
// données, serveur et session du compte par défaut : `e2e/servers.ts` et
// `e2e/global-setup.ts`, comme pour la suite ordinaire (`playwright.config.ts`).
import { defineConfig } from "@playwright/test";
import { apiServer, BASE_URL, defaultStorageState, env, WEB, WEB_PORT } from "./e2e/servers";

const VITE = "node node_modules/vite/bin/vite.js";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "budgets.spec.ts",
  timeout: 60_000,
  // Une mesure à la fois, sur une machine au calme.
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"]],
  globalSetup: "./e2e/global-setup.ts",
  use: {
    baseURL: BASE_URL,
    storageState: defaultStorageState(),
    headless: true,
    trace: "off",
  },
  webServer: [
    apiServer({ release: true }),
    {
      command: `${VITE} build && ${VITE} preview --port ${WEB_PORT} --strictPort`,
      cwd: WEB,
      url: `http://localhost:${WEB_PORT}`,
      reuseExistingServer: false,
      timeout: 300_000,
      env,
    },
  ],
});
