// L'attente bornée des tests de composants (`until`, `hooks.ts`) : elle s'arrête dès que l'hôte du
// test quitte le document, avant de relire sa condition. Une attente qui survit au délai de Vitest
// (le test est coupé, son `afterEach` a retiré l'hôte) ne peut donc pas être satisfaite par le test
// suivant, ni rendre ensuite dans sa racine.
import { expect, test } from "vitest";
import { until } from "./hooks";

test("until_resolves_once_its_condition_holds", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  let ready = false;
  setTimeout(() => (ready = true), 30);
  await until(host, () => ready, "prêt");
  expect(ready).toBe(true);
  host.remove();
});

test("until_stops_once_the_host_has_left_the_document_even_if_the_condition_comes_true", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  let ready = false;
  const waiting = until(host, () => ready, "la fenêtre");
  // Le test est fini (son hôte retiré) ; la condition, lue par le test suivant, devient vraie.
  host.remove();
  ready = true;
  await expect(waiting).rejects.toThrow("test fini : la fenêtre");
});

test("until_says_what_it_waited_for_at_its_bound", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  await expect(until(host, () => false, "rien", 50)).rejects.toThrow("toujours pas : rien");
  host.remove();
});
