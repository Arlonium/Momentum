// Les données du menu Partager d'une page : ses accès, tels que le serveur les donne (`GET
// /api/v1/pages/{id}/access`), et les changements qu'on y fait. Le serveur fait foi.
//
// - Les accès sont lus quand le menu s'ouvre, et relus à chaque ouverture ; ce qu'on savait
//   reste montré pendant la relecture, et après une relecture qui échoue.
// - Un changement (niveau d'une personne, retrait, accès général) rend les accès tels qu'ils
//   sont devenus : ils sont montrés, sans relecture. Un seul changement à la fois.
// - Un refus (`409 last_full_access`, `409 inherited_access`, `403 forbidden`…) s'affiche tel
//   que le serveur le dit, et les accès sont relus : l'écran revient à l'état du serveur.
// - `204`, sans corps : le changement est fait, et la personne n'a plus l'accès complet. De
//   même quand le serveur ne lui rend plus les accès (`403`), ou plus la page (`404`) :
//   `onAccessLost`, avec le refus qui l'a révélé s'il y en a un. Le menu n'a plus rien à
//   montrer ; l'application relit la page.
// - « Révoquer » refusé par `404` : l'invitation n'attend plus (acceptée, échue, révoquée
//   ailleurs), ce que la liste relue montre sans un mot ; ou la personne ne peut plus la gérer,
//   et le serveur répond de même. Le refus est alors dit : par la relecture refusée à son tour,
//   ou dans le menu si la liste relue porte encore l'invitation.
// - Une invitation montre son lien une fois : le serveur ne garde que le hachage de son jeton.
//   Le lien reste à l'écran tant que l'invitation attend, même si la relecture de la liste
//   échoue ; il part quand elle n'attend plus (acceptée, révoquée).
//
// Deux réponses ne se croisent pas : seule la plus récente demandée est montrée.
import type { Level, PageAccess } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ApiError,
  getAccess,
  inviteToPage,
  isSessionLost,
  removeUserAccess,
  revokeInvitation,
  setGeneralAccess,
  setUserLevel,
} from "../api";
import { messageOf } from "../session/forms";
import { invitationLink } from "./levels";

/** Une invitation tout juste créée, avec son lien : montré tant qu'elle attend. */
export type CreatedLink = { id: string; email: string; level: Level; link: string };

export type Share = {
  /** Les accès de la page ; `null` : pas encore lus. */
  access: PageAccess | null;
  /** Une lecture des accès est en route. */
  reading: boolean;
  /** La dernière lecture des accès a échoué : le message du serveur. */
  readError: string | null;
  /** Le refus du dernier changement, tel que le serveur le dit. */
  error: string | null;
  /**
   * Ce qui est en route, sinon `null` : `invite`, `general`, `user:<id>` ou
   * `invitation:<id>`. Rien d'autre ne part pendant ce temps.
   */
  busy: string | null;
  /** L'invitation qui vient d'être créée, avec son lien. */
  created: CreatedLink | null;
  /** Le refus de la dernière invitation, tel que le serveur le dit. */
  inviteError: string | null;
  /** Relit les accès. */
  reload: () => Promise<void>;
  /** Invite `email` sur la page ; `true` : l'invitation est créée. */
  invite: (email: string, level: Level) => Promise<boolean>;
  setLevel: (userId: string, level: Level) => Promise<void>;
  remove: (userId: string) => Promise<void>;
  /** `null` : seulement les personnes nommées. */
  setGeneral: (level: Level | null) => Promise<void>;
  revoke: (invitationId: string) => Promise<void>;
};

type Callbacks = {
  /** Les accès de la page ont changé : la barre latérale est à relire. */
  onChanged: () => void;
  /**
   * La personne n'a plus l'accès complet, ou plus la page. `refusal` : le refus du serveur qui
   * l'a révélé, s'il y en a un.
   */
  onAccessLost: (refusal: string | null) => void;
};

/** Le serveur ne rend plus les accès à cette personne, ou plus la page. */
const isDenied = (e: unknown): e is ApiError =>
  e instanceof ApiError && (e.status === 403 || e.status === 404);

/** Le refus d'une révocation, quand le serveur n'en dit rien d'utile (« introuvable »). */
const NOT_REVOKED = "Cette invitation n'a pas pu être révoquée.";

/**
 * Ce qu'une relecture refusée à son tour rend à l'application : le refus qui l'a demandée,
 * ou celui que choisit `(denied)`, le refus de la relecture elle-même.
 */
type Refusal = string | null | ((denied: ApiError) => string);

export function useShare(pageId: string, open: boolean, callbacks: Callbacks): Share {
  const [access, setAccess] = useState<PageAccess | null>(null);
  const [reading, setReading] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [created, setCreated] = useState<CreatedLink | null>(null);
  const [inviteError, setInviteError] = useState<string | null>(null);
  /** Le rang de la dernière réponse demandée : une plus ancienne qui arrive après est jetée. */
  const asked = useRef(0);
  /** Le nombre de lectures en route. */
  const pending = useRef(0);
  /** Un changement est en route. */
  const working = useRef(false);
  // Les rappels du dernier rendu.
  const latest = useRef(callbacks);
  latest.current = callbacks;

  /** Montre les accès rendus par le serveur, si c'est bien la dernière réponse demandée. */
  const show = useCallback((rank: number, next: PageAccess) => {
    if (rank !== asked.current) return;
    setAccess(next);
    setReadError(null);
    // Le lien d'une invitation qui n'attend plus ne vaut plus rien.
    setCreated((link) => (link && next.invitations.some((i) => i.id === link.id) ? link : null));
  }, []);

  /**
   * Relit les accès, et les rend s'ils sont montrés ; `refusal` : le refus qui vient de
   * demander cette relecture.
   */
  const read = useCallback(
    async (refusal: Refusal): Promise<PageAccess | null> => {
      const rank = ++asked.current;
      pending.current += 1;
      setReading(true);
      try {
        const next = await getAccess(pageId);
        show(rank, next);
        return rank === asked.current ? next : null;
      } catch (e) {
        if (rank !== asked.current || isSessionLost(e)) return null;
        if (isDenied(e)) {
          latest.current.onAccessLost(typeof refusal === "function" ? refusal(e) : refusal);
        } else setReadError(messageOf(e));
        return null;
      } finally {
        pending.current -= 1;
        setReading(pending.current > 0);
      }
    },
    [pageId, show],
  );
  const reload = useCallback(async () => {
    await read(null);
  }, [read]);

  useEffect(() => {
    if (!open) return;
    void reload();
    // Le menu fermé : la réponse en route ne compte plus.
    return () => {
      asked.current++;
    };
  }, [open, reload]);

  /** Commence `what`, si rien d'autre n'est en route. */
  const begin = (what: string): boolean => {
    if (working.current) return false;
    working.current = true;
    setBusy(what);
    setError(null);
    return true;
  };
  const end = () => {
    working.current = false;
    setBusy(null);
  };

  const change = useCallback(
    async (what: string, send: () => Promise<PageAccess | null>) => {
      if (!begin(what)) return;
      const rank = ++asked.current;
      try {
        const next = await send();
        if (next === null) {
          // `204` : fait, mais la personne n'a plus l'accès complet.
          latest.current.onAccessLost(null);
          return;
        }
        show(rank, next);
        latest.current.onChanged();
      } catch (e) {
        // Session perdue : la connexion est demandée par-dessus l'application.
        if (isSessionLost(e)) return;
        const refusal = messageOf(e);
        setError(refusal);
        // Les accès tels que le serveur les voit, à la place de ce que l'écran croyait.
        await read(refusal);
      } finally {
        end();
      }
    },
    [show, read],
  );

  const setLevel = useCallback(
    (userId: string, level: Level) =>
      change(`user:${userId}`, () => setUserLevel(pageId, userId, level)),
    [change, pageId],
  );
  const remove = useCallback(
    (userId: string) => change(`user:${userId}`, () => removeUserAccess(pageId, userId)),
    [change, pageId],
  );
  const setGeneral = useCallback(
    (level: Level | null) => change("general", () => setGeneralAccess(pageId, level)),
    [change, pageId],
  );

  const invite = useCallback(
    async (email: string, level: Level) => {
      if (!begin("invite")) return false;
      setInviteError(null);
      try {
        const { id, token } = await inviteToPage(pageId, { email, level });
        // L'adresse, telle que le serveur la garde : rognée, en minuscules.
        setCreated({ id, email: email.trim().toLowerCase(), level, link: invitationLink(token) });
      } catch (e) {
        if (!isSessionLost(e)) {
          const refusal = messageOf(e);
          setInviteError(refusal);
          if (isDenied(e)) await read(refusal);
        }
        end();
        return false;
      }
      end();
      // La nouvelle invitation, sans son jeton, dans la liste de celles qui attendent. Si la
      // relecture échoue, l'invitation existe quand même, et son lien reste montré.
      await reload();
      return true;
    },
    [pageId, read, reload],
  );

  const revoke = useCallback(
    async (invitationId: string) => {
      if (!begin(`invitation:${invitationId}`)) return;
      let notFound = false;
      try {
        await revokeInvitation(invitationId);
      } catch (e) {
        if (isSessionLost(e)) {
          end();
          return;
        }
        notFound = e instanceof ApiError && e.status === 404;
        if (!notFound) setError(messageOf(e));
      }
      // Réussie ou refusée : la liste telle que le serveur la voit.
      if (!notFound) await read(null);
      else {
        // Le serveur dit pourquoi il ne rend plus les accès (`403`) ; d'une page qu'il ne rend
        // plus non plus, il ne dit qu'« introuvable ».
        const next = await read((denied) =>
          denied.status === 403 ? messageOf(denied) : NOT_REVOKED,
        );
        // Si l'invitation n'attend plus, la liste relue le montre, et il n'y a rien à dire.
        if (next?.invitations.some((i) => i.id === invitationId)) setError(NOT_REVOKED);
      }
      end();
    },
    [read],
  );

  return {
    access,
    reading,
    readError,
    error,
    busy,
    created,
    inviteError,
    reload,
    invite,
    setLevel,
    remove,
    setGeneral,
    revoke,
  };
}
