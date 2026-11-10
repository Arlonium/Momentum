// Une lecture tombée sur une session perdue repart quand le compte revient, et seulement
// alors.
import type { Me } from "@mementum/protocol";
import { expect, test } from "vitest";
import { renderHook, settle } from "../test/hooks";
import { space } from "../test/pages";
import { useRetryAfterLogin } from "./useRetryAfterLogin";

/** Le compte tel que le serveur le rend : un objet neuf à chaque lecture. */
const me = (): Me => ({
  user: { id: "u", email: "camille@exemple.fr", name: "Camille", instanceAdmin: false },
  spaces: [space("perso")],
});

test("lost_read_is_made_again_when_the_account_is_back", async () => {
  const retry = await renderHook(useRetryAfterLogin, me());
  expect(retry.current.retries).toBe(0);
  const { sessionLost } = retry.current;

  // Le compte est relu sans qu'aucune lecture ait été perdue : rien n'est à refaire.
  await retry.rerender(me());
  expect(retry.current.retries).toBe(0);

  // Une lecture répond `401` : la connexion est demandée par-dessus l'application, et rien
  // ne repart tant que le compte n'est pas revenu.
  await settle(() => sessionLost());
  expect(retry.current.retries).toBe(0);
  // Le compte revient : les lectures qui dépendent de `retries` repartent, une fois.
  await retry.rerender(me());
  expect(retry.current.retries).toBe(1);
  await retry.rerender(me());
  expect(retry.current.retries).toBe(1);

  // Deux lectures perdues pendant la même coupure : une seule reprise.
  await settle(() => {
    sessionLost();
    sessionLost();
  });
  await retry.rerender(me());
  expect(retry.current.retries).toBe(2);
  // Le rappel garde son identité : il est dans les dépendances des lectures.
  expect(retry.current.sessionLost).toBe(sessionLost);
});
