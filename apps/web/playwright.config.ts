// Tests de bout en bout : Playwright lance le vrai serveur (`mementum serve`, sur un
// dossier de données temporaire), puis Vite, et les tests passent par le mandataire de
// Vite comme un navigateur. Ports, contrôle des ports pris, dossier de données et serveur :
// `e2e/servers.ts`. Chaque test porte la session du compte par défaut, créé par
// `e2e/global-setup.ts` (`storageState`).
//
// Les budgets du navigateur (`e2e/budgets.spec.ts`) n'en font pas partie : ils se mesurent
// sur un build de production et un serveur `--release` (`playwright.budgets.config.ts`,
// Ruling R38).
import { defineConfig } from "@playwright/test";
import {
  apiServer,
  BASE_URL,
  defaultStorageState,
  E2E_VERSION_EVERY_SECS,
  env,
  WEB,
  WEB_PORT,
} from "./e2e/servers";

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
  globalSetup: "./e2e/global-setup.ts",
  use: {
    baseURL: BASE_URL,
    storageState: defaultStorageState(),
    headless: true,
    trace: "off",
  },
  webServer: [
    // Une version de page après une seconde d'écritures (R135), au lieu de dix minutes : les
    // e2e de l'historique en voient une sans attendre. Pas les budgets (Ruling R144).
    apiServer({ release: false, versionEverySecs: E2E_VERSION_EVERY_SECS }),
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
