// Les personnes que le menu `@` propose (Task C4, R251, R617). Pour un membre de l'espace : ses
// membres et ses invités, par la route existante (`GET …/members`), lus quand le menu le demande,
// jamais avant, et gardés une minute. Pour un invité, à qui cette route est fermée, ou si la lecture
// échoue : les comptes qui ont la page ouverte (la trame `roster` de la session). Rien d'autre qu'un
// id et un nom n'en sort : pas d'adresse.
//
// La liste gardée vit dans ce module, avec l'onglet : un autre compte recharge l'application (R115).
import type { Role } from "@mementum/protocol";
import { getMembers } from "../api";
import type { RosterUser } from "../editor/collab";

/** Une personne à mentionner. */
export type Person = { id: string; name: string };

/** Combien de temps la liste lue sert sans être relue. */
export const PEOPLE_KEPT_MS = 60_000;

/** Combien de temps le menu attend la lecture avant de se contenter de qui est sur la page. */
export const PEOPLE_WAIT_MS = 2_000;

/** La dernière liste lue, et son espace. */
let kept: { spaceId: string; at: number; people: Person[] } | null = null;
/** La lecture en route, et son espace. */
let reading: { spaceId: string; answer: Promise<Person[] | null> } | null = null;

/** Oublie la liste gardée (les tests ; rien d'autre ne l'appelle). */
export function forgetPeople(): void {
  kept = null;
  reading = null;
}

/** Lit les membres de `spaceId` ; `null` si la lecture échoue (refus, réseau, session perdue). */
function read(spaceId: string): Promise<Person[] | null> {
  if (reading !== null && reading.spaceId === spaceId) return reading.answer;
  const answer = getMembers(spaceId).then(
    (members) => {
      const people = members.map((m) => ({ id: m.userId, name: m.name }));
      if (reading?.answer === answer) kept = { spaceId, at: Date.now(), people };
      return people;
    },
    () => null,
  );
  const mine = { spaceId, answer };
  reading = mine;
  void answer.then(() => {
    if (reading === mine) reading = null;
  });
  return answer;
}

/**
 * Les personnes à proposer dans l'espace `spaceId` au compte de rôle `role`, dont la page ouverte
 * est regardée par `roster`.
 */
export async function loadPeople(spaceId: string, role: Role, roster: readonly RosterUser[]): Promise<Person[]> {
  const present = () => roster.map((user) => ({ id: user.id, name: user.name }));
  if (role === "guest") return present();
  if (kept !== null && kept.spaceId === spaceId && Date.now() - kept.at < PEOPLE_KEPT_MS) return kept.people;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), PEOPLE_WAIT_MS);
  });
  const people = await Promise.race([read(spaceId), late]);
  clearTimeout(timer);
  return people ?? present();
}
