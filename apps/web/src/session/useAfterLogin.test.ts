// Ce qui a été refusé par une session perdue (un titre, une connexion de synchro) attend le
// retour du compte, et ne repart qu'alors.
import type { Me } from "@mementum/protocol";
import { expect, test } from "vitest";
import { renderHook, settle } from "../test/hooks";
import { space } from "../test/pages";
import { useAfterLogin } from "./useAfterLogin";

/** Le compte tel que le serveur le rend : un objet neuf à chaque lecture. */
const me = (): Me => ({
  user: { id: "u", email: "camille@exemple.fr", name: "Camille", instanceAdmin: false },
  spaces: [space("perso")],
});

test("waiters_resume_when_the_account_is_back", async () => {
  const hook = await renderHook(useAfterLogin, me());
  const afterLogin = hook.current;
  let resumed = 0;
  void afterLogin().then(() => resumed++);
  void afterLogin().then(() => resumed++);
  // Tant que le compte n'est pas revenu, rien ne repart.
  await settle();
  expect(resumed).toBe(0);
  // Le compte revient (reconnexion) : tous ceux qui attendaient repartent, une fois.
  await hook.rerender(me());
  await settle();
  expect(resumed).toBe(2);
  await hook.rerender(me());
  await settle();
  expect(resumed).toBe(2);
  // Une attente posée après ce retour attend le suivant.
  void afterLogin().then(() => resumed++);
  await settle();
  expect(resumed).toBe(2);
  await hook.rerender(me());
  await settle();
  expect(resumed).toBe(3);
  // La fonction garde son identité : elle est dans les dépendances de ceux qui s'en servent.
  expect(hook.current).toBe(afterLogin);
});

test("nobody_resumes_once_unmounted", async () => {
  const hook = await renderHook(useAfterLogin, me());
  let resumed = 0;
  void hook.current().then(() => resumed++);
  // Un autre compte se connecte par-dessus : l'application est démontée (puis l'onglet
  // rechargé, R115). Ce que le premier attendait d'envoyer ne part jamais sous le nom du
  // second.
  await hook.unmount();
  await settle();
  expect(resumed).toBe(0);
});
