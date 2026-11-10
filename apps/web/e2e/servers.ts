// Ce que partagent les deux configurations de Playwright : la suite ordinaire
// (`playwright.config.ts`, Vite en développement et serveur debug) et les budgets du
// navigateur (`playwright.budgets.config.ts`, build de production et serveur `--release`,
// Ruling R38).
//
// Ports (Ruling « Setup » du lot 1) : `MEMENTUM_E2E_WEB_PORT` (défaut 5173) et
// `MEMENTUM_E2E_API_PORT` (défaut 8080). Un serveur déjà présent n'est jamais réutilisé
// (`reuseExistingServer: false`) : en phase 0, un autre projet occupait 5173 et les tests
// visaient son application. Les configurations vérifient donc d'abord que les deux ports
// sont libres, et s'arrêtent sinon en nommant le port pris.
//
// Sessions : `global-setup.ts` crée le compte par défaut et enregistre sa session
// (`defaultStorageState()`), que portent tous les contextes des tests ; `e2e/helpers.ts`
// crée les autres comptes (`signUpAccount`).
import type { PlaywrightTestConfig } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const WEB_PORT = Number(process.env.MEMENTUM_E2E_WEB_PORT || 5173);
export const API_PORT = Number(process.env.MEMENTUM_E2E_API_PORT || 8080);
/** Racine du dépôt (workspace Cargo). */
export const REPO = fileURLToPath(new URL("../../..", import.meta.url));
/** `apps/web`. */
export const WEB = fileURLToPath(new URL("..", import.meta.url));
/** L'application, telle que les tests l'ouvrent (par le mandataire de Vite pour l'API). */
export const BASE_URL = `http://localhost:${WEB_PORT}`;

/** Environnement des serveurs lancés : Vite lit les ports pour son mandataire. */
export const env = {
  MEMENTUM_E2E_WEB_PORT: String(WEB_PORT),
  MEMENTUM_E2E_API_PORT: String(API_PORT),
};

/**
 * Adresse de la boucle locale où `port` est déjà pris, ou `null` s'il est libre. Le test
 * tourne dans un `node` à part pour rester synchrone. Il essaie de se connecter, puis
 * d'écouter, sur 127.0.0.1 et sur [::1] : Vite écoute souvent sur [::1] seulement.
 */
function takenOn(port: number): string | null {
  const probe = `
    const net = require("node:net");
    const port = Number(process.argv[1]);
    const connects = (host) => new Promise((done) => {
      const s = net.connect({ port, host });
      s.setTimeout(1000);
      s.once("connect", () => { s.destroy(); done(true); });
      s.once("timeout", () => { s.destroy(); done(false); });
      s.once("error", () => done(false));
    });
    const listens = (host) => new Promise((done) => {
      const s = net.createServer();
      s.once("error", (e) => done(e.code === "EADDRNOTAVAIL" || e.code === "EAFNOSUPPORT"));
      s.listen({ port, host }, () => s.close(() => done(true)));
    });
    (async () => {
      for (const host of ["127.0.0.1", "::1"]) {
        if ((await connects(host)) || !(await listens(host))) {
          process.stdout.write(host);
          return;
        }
      }
    })();
  `;
  const out = execFileSync(process.execPath, ["-e", probe, String(port)], { encoding: "utf8" });
  return out.trim() || null;
}

const RUN_DIR_VAR = "MEMENTUM_E2E_INTERNAL_RUN_DIR";

/**
 * Le dossier de cette exécution : `data/`, les données du serveur, et `accounts/`, les
 * comptes de test et leurs sessions. La configuration est relue par chaque processus de
 * test, et parfois par le processus principal lui-même (modes UI et watch), alors que nos
 * serveurs tiennent déjà les ports. La première lecture vérifie les ports (et arrête tout
 * si l'un est pris), crée le dossier et le note dans l'environnement, dont héritent les
 * processus de test.
 */
export function prepareServers(): string {
  let runDir = process.env[RUN_DIR_VAR] ?? "";
  if (runDir || process.env.TEST_WORKER_INDEX !== undefined) return runDir;
  for (const [name, port] of [
    ["MEMENTUM_E2E_WEB_PORT", WEB_PORT],
    ["MEMENTUM_E2E_API_PORT", API_PORT],
  ] as const) {
    const host = takenOn(port);
    if (host) {
      console.error(
        `Le port ${port} est déjà pris (${host.includes(":") ? `[${host}]` : host}). ` +
          "Les tests e2e ne réutilisent jamais un serveur existant : libère ce port, " +
          `ou choisis-en un autre avec ${name}=<port>.`,
      );
      process.exit(1);
    }
  }
  runDir = mkdtempSync(join(tmpdir(), "mementum-e2e-"));
  mkdirSync(join(runDir, "data"));
  mkdirSync(join(runDir, "accounts"));
  process.env[RUN_DIR_VAR] = runDir;
  // Après l'arrêt des serveurs ; un fichier encore verrouillé reste dans le dossier temporaire.
  process.on("exit", () => {
    try {
      rmSync(runDir, { recursive: true, force: true, maxRetries: 3 });
    } catch {
      // dossier temporaire du système : sans conséquence
    }
  });
  return runDir;
}

/** Le dossier des comptes de test et de leurs sessions (`storageState`). */
export function accountsDir(): string {
  return join(prepareServers(), "accounts");
}

/** La session du compte par défaut, créée par `global-setup.ts`. */
export function defaultStorageState(): string {
  return join(accountsDir(), "default.storage.json");
}

type WebServer = Exclude<NonNullable<PlaywrightTestConfig["webServer"]>, unknown[]>;

/**
 * Le serveur Mementum des tests, sur le dossier de données de cette exécution : compilé en
 * debug pour la suite ordinaire, en release pour les budgets (`release`, Ruling R38).
 * Inscriptions ouvertes (`MEMENTUM_SIGNUP=open`) : chaque test crée ses comptes.
 */
export function apiServer({ release }: { release: boolean }): WebServer {
  const dataDir = join(prepareServers(), "data");
  return {
    // La première compilation (en release surtout) peut être longue : délai généreux.
    command: `cargo run ${release ? "--release " : ""}-p mementum-cli -- serve --bind 127.0.0.1:${API_PORT} --data-dir "${dataDir}"`,
    cwd: REPO,
    url: `http://127.0.0.1:${API_PORT}/health`,
    reuseExistingServer: false,
    timeout: 900_000,
    stdout: "ignore",
    stderr: "pipe",
    env: { ...env, MEMENTUM_SIGNUP: "open" },
  };
}
