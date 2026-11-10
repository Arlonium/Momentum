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
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const WEB_PORT = Number(process.env.MEMENTUM_E2E_WEB_PORT || 5173);
export const API_PORT = Number(process.env.MEMENTUM_E2E_API_PORT || 8080);
/** Racine du dépôt (workspace Cargo). */
export const REPO = fileURLToPath(new URL("../../..", import.meta.url));
/** `apps/web`. */
export const WEB = fileURLToPath(new URL("..", import.meta.url));

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

const DATA_DIR_VAR = "MEMENTUM_E2E_INTERNAL_DATA_DIR";

/**
 * Le dossier de données du serveur de cette exécution. La configuration est relue par
 * chaque processus de test, et parfois par le processus principal lui-même (modes UI et
 * watch), alors que nos serveurs tiennent déjà les ports. La première lecture vérifie les
 * ports (et arrête tout si l'un est pris), crée le dossier et le note dans
 * l'environnement, dont héritent les processus de test.
 */
export function prepareServers(): string {
  let dataDir = process.env[DATA_DIR_VAR] ?? "";
  if (dataDir || process.env.TEST_WORKER_INDEX !== undefined) return dataDir;
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
  dataDir = mkdtempSync(join(tmpdir(), "mementum-e2e-"));
  process.env[DATA_DIR_VAR] = dataDir;
  // Après l'arrêt des serveurs ; un fichier encore verrouillé reste dans le dossier temporaire.
  process.on("exit", () => {
    try {
      rmSync(dataDir, { recursive: true, force: true, maxRetries: 3 });
    } catch {
      // dossier temporaire du système : sans conséquence
    }
  });
  return dataDir;
}
