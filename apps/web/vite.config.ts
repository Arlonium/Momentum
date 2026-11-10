import react from "@vitejs/plugin-react";
import { createLogger, defineConfig } from "vite";

// Ports (Ruling « Setup » du lot 1) : 5173 pour Vite et 8080 pour le serveur, sauf si
// l'environnement en donne d'autres (sur le poste de développement, 5173 est souvent pris).
const WEB_PORT = Number(process.env.MEMENTUM_E2E_WEB_PORT || 5173);
const API_PORT = Number(process.env.MEMENTUM_E2E_API_PORT || 8080);
const API = `http://127.0.0.1:${API_PORT}`;

// Un onglet qui se ferme ou se recharge coupe sa WebSocket pendant que le serveur écrit :
// le mandataire le journalise en erreur. Ce n'en est pas une ; les autres restent affichées.
const CLIENT_GONE = new Set(["ECONNABORTED", "ECONNRESET", "EPIPE"]);
const logger = createLogger();
const logError = logger.error.bind(logger);
logger.error = (message, options) => {
  const code = (options?.error as NodeJS.ErrnoException | null | undefined)?.code;
  if (message.includes("ws proxy") && code && CLIENT_GONE.has(code)) return;
  logError(message, options);
};

// Pas de `changeOrigin` : le serveur ne répond qu'à `Host: localhost`, `127.0.0.1` ou
// `[::1]` (Ruling R31), et le mandataire doit garder `Host: localhost:<port>`. L'`Origin`
// de la page (`http://localhost:<port>`) passe aussi telle quelle.
const proxy = {
  "/api/": { target: API },
  "/sync/": { target: API, ws: true },
};

export default defineConfig({
  plugins: [react()],
  customLogger: logger,
  server: {
    port: WEB_PORT,
    strictPort: true,
    proxy,
  },
  // `vite preview` sert le build de production, avec le même mandataire : les budgets du
  // navigateur s'y mesurent (`playwright.budgets.config.ts`, Ruling R38).
  preview: {
    proxy,
  },
});
