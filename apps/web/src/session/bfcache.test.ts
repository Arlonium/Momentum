// Un document rendu par le cache arrière-avant (Précédent après une déconnexion) se recharge.
// Playwright lance Chromium sans ce cache, et le client WebSocket de Vite en développement
// en exclut de toute façon la page : le test porte sur l'écouteur.
import { expect, test, vi } from "vitest";
import { reloadWhenRestored } from "./bfcache";

function pageShow(persisted: boolean): Event {
  return new PageTransitionEvent("pageshow", { persisted });
}

test("restored_page_reloads", () => {
  const reload = vi.fn();
  const stop = reloadWhenRestored(window, reload);
  // Un chargement ordinaire : rien.
  window.dispatchEvent(pageShow(false));
  expect(reload).not.toHaveBeenCalled();
  // Rendu par le cache : rechargé.
  window.dispatchEvent(pageShow(true));
  expect(reload).toHaveBeenCalledTimes(1);
  // L'écouteur retiré ne recharge plus.
  stop();
  window.dispatchEvent(pageShow(true));
  expect(reload).toHaveBeenCalledTimes(1);
});
