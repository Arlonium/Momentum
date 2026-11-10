// Ce que montrent les réglages d'un espace : ses membres (`GET /spaces/{id}/members`), et ses
// invitations en attente pour qui les gère (`GET /spaces/{id}/invitations`). Le tout est lu à
// l'ouverture des réglages, et relu après chaque action et après chaque refus (`reload`) :
// l'écran montre l'état du serveur, pas celui qu'il croyait.
//
// Deux relectures peuvent se suivre de près : seule la réponse de la plus récente compte, une
// plus ancienne qui arrive après elle est jetée. Une relecture qui échoue garde à l'écran ce
// qui a déjà été lu, avec le message du serveur (`reloadError`), que la relecture suivante qui
// réussit efface. Le rôle de la personne connectée est lu avec la liste des membres, pas pris
// de la session, qui peut dater. Une lecture tombée sur une session perdue ne dit rien, et se
// signale (`onSessionLost`) : elle est refaite quand le compte revient.
import type { Member, PendingInvitation } from "@mementum/protocol";
import { useCallback, useRef, useState } from "react";
import { ApiError, getMembers, getSpaceInvitations, isSessionLost } from "../api";
import { messageOf } from "../session/forms";
import { isManager } from "./roles";

export type Load =
  | { name: "loading" }
  /** Aucun rôle dans cet espace, ou pas d'espace : le serveur répond `404`. */
  | { name: "missing" }
  /** La première lecture a échoué : rien à montrer d'autre. */
  | { name: "failed"; message: string }
  | { name: "ready"; members: Member[]; invitations: PendingInvitation[] };

export type SpaceSettingsData = {
  load: Load;
  /** La dernière relecture a échoué ; ce qui avait été lu reste à l'écran. */
  reloadError: string | null;
  /** Relit tout depuis le serveur. La promesse est tenue quand cette lecture est finie. */
  reload: () => Promise<void>;
};

const roleOf = (members: Member[], self: string) => members.find((m) => m.userId === self)?.role;

/**
 * `self` : le compte connecté. `onForbidden` : la liste des membres lui est refusée, c'est un
 * invité de l'espace d'après le serveur, et les réglages ne sont pas pour lui.
 * `onSessionLost` : une lecture a répondu `401 unauthenticated`, et n'a rien lu.
 */
export function useSpaceSettings(
  spaceId: string,
  self: string,
  onForbidden: () => void,
  onSessionLost: () => void,
): SpaceSettingsData {
  const [load, setLoad] = useState<Load>({ name: "loading" });
  const [reloadError, setReloadError] = useState<string | null>(null);
  /** Le rang de la dernière lecture lancée : la réponse d'une plus ancienne est jetée. */
  const reads = useRef(0);
  const forbidden = useRef(onForbidden);
  forbidden.current = onForbidden;
  const sessionLost = useRef(onSessionLost);
  sessionLost.current = onSessionLost;
  /** Ce qui est à l'écran, à l'instant : pour une lecture qui a attendu le serveur. */
  const shown = useRef<Load["name"]>("loading");
  const show = useCallback((next: Load) => {
    shown.current = next.name;
    setLoad(next);
  }, []);

  const reload = useCallback(async () => {
    const read = ++reads.current;
    const outdated = () => read !== reads.current;
    try {
      let members = await getMembers(spaceId);
      if (outdated()) return;
      let invitations: PendingInvitation[] = [];
      if (isManager(roleOf(members, self))) {
        try {
          invitations = await getSpaceInvitations(spaceId);
        } catch (e) {
          // Le rôle a baissé entre les deux appels : ce n'est pas un invité, les membres
          // relus disent ce qu'il est devenu.
          if (!(e instanceof ApiError && e.status === 403)) throw e;
          members = await getMembers(spaceId);
        }
        if (outdated()) return;
      }
      show({ name: "ready", members, invitations });
      setReloadError(null);
    } catch (e) {
      if (outdated()) return;
      // Session perdue : la connexion est demandée par-dessus ; l'écran garde ce qu'il a, et
      // sera relu au retour du compte.
      if (isSessionLost(e)) sessionLost.current();
      else if (e instanceof ApiError && e.status === 404) show({ name: "missing" });
      // La liste des membres refusée : un invité, d'après le serveur (la session le disait
      // encore membre).
      else if (e instanceof ApiError && e.status === 403) forbidden.current();
      // Ce qui a déjà été lu reste : seule une première lecture qui échoue le remplace.
      else if (shown.current === "ready") setReloadError(messageOf(e));
      else show({ name: "failed", message: messageOf(e) });
    }
  }, [spaceId, self, show]);

  return { load, reloadError, reload };
}
