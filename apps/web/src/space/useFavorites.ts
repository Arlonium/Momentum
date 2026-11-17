// Les favoris du compte dans l'espace affiché (R254), tels que le serveur les rend
// (`GET /api/v1/spaces/{id}/favorites`) : des identifiants, dans l'ordre où le compte les a
// ajoutés, déjà triés par les droits (une page qu'il ne voit plus, ou à la corbeille, n'y est pas).
// La barre latérale les montre filtrés par l'arbre (`favoritePages`) : entre deux lectures, une
// page partie de l'arbre n'est plus montrée, et revient avec lui.
//
// Quand les lire. Jamais à l'ouverture d'une page : la première lecture part à la fin de la
// première lecture de l'arbre de l'espace (`SpaceTree.onRead`), rien sur le chemin de l'ouverture
// (budgets du §5.10). Puis après **chaque** lecture de l'arbre qui aboutit, même quand elle rend
// le même arbre : un favori n'avance pas la version de l'espace (R254), et c'est la lecture de
// l'arbre (retour sur l'onglet, changement dans l'espace, minuteur de repli) qui l'apporte aux
// autres onglets du compte. Et après chaque favori posé ou retiré d'ici.
//
// Les relectures passent par `rereads.ts` : jamais annulées en route ; une demande faite pendant
// une lecture est servie une fois, à sa fin ; une lecture qui échoue garde ce qui était affiché, et
// c'est la prochaine lecture de l'arbre qui la refait ; une lecture qui trouve la session perdue
// est refaite au retour du compte (`afterLogin`), sans autre demande. Tout est lié à l'espace
// affiché : remis à zéro quand il change, et plus rien après le démontage.
//
// Poser ou retirer un favori (`toggle`) se voit tout de suite : la page est ajoutée à la fin de la
// liste montrée, ou en est retirée, avant la réponse. Les demandes partent l'une après l'autre,
// dans l'ordre des gestes (le serveur les reçoit ainsi dans cet ordre). Le geste reste montré
// jusqu'à ce qu'une lecture partie après sa réponse fasse foi : une lecture partie avant ne le
// reprend pas. Refusé, il est défait (la page revient à sa place, ou repart ; la défaite prend un
// rang neuf dans `toggles`, que la barre ne gèle pas, R467), le refus se dit tel
// que le serveur le dit (`error`, jusqu'au geste suivant ou à un autre espace) et part à
// `onRefused` (la barre latérale est à relire : la page n'est plus visible, ou n'existe plus). Une
// session perdue n'est pas un refus : le geste est défait sans message, la connexion est demandée
// par-dessus l'application, et rien ne repart seul au retour du compte.
//
// Une réponse perdue (`answerLost` : coupée en route, ou rendue par un mandataire) n'est pas un refus
// non plus : le serveur a pu poser le favori (R759). La liste est relue avant de rien dire, dans la
// file des gestes. Elle montre ce qui était demandé : le geste est confirmé, sans un mot. Elle montre
// le contraire : il n'a pas été fait, et se dit comme un refus (`NOT_DONE`). Elle ne se lit pas : le
// geste est défait (la dernière liste lue fait foi), et le refus garde son doute.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { answerLost, ApiError, getFavorites, isSessionLost, setFavorite } from "../api";
import { scheduleRereads, SESSION_LOST, waitForAccount } from "../rereads";
import { messageOf } from "../session/forms";
import type { SpaceTree } from "./useSpaceTree";

export type FavoritesOptions = {
  /**
   * Tenue quand le compte revient, après une session perdue (`useAfterLogin`) : la liste est alors
   * relue. Sans elle, une lecture refusée par `401` attend la prochaine lecture de l'arbre.
   */
  afterLogin?: () => Promise<void>;
  /**
   * Le serveur a refusé de poser (`on`) ou de retirer le favori de `pageId` : la page a pu
   * changer (plus visible, supprimée), ce qu'on en montre est à relire.
   */
  onRefused?: (error: ApiError, pageId: string, on: boolean) => void;
};

export type FavoriteList = {
  /**
   * Les pages favorites de l'espace affiché, dans l'ordre où le compte les a ajoutées, avec les
   * gestes d'ici pas encore relus ; `[]` tant que la liste n'est pas lue.
   */
  pages: string[];
  /**
   * La liste de l'espace affiché a été lue au moins une fois : `pages` dit ce que le serveur en
   * sait, pas seulement qu'on attend (la barre latérale ne gèle « Favoris » qu'à partir de là).
   */
  loaded: boolean;
  /**
   * Le rang du dernier geste fait d'ici sur chaque page de l'espace affiché (refusé compris) : la
   * barre latérale ne gèle pas ce que la personne fait elle-même (R411), d'où que vienne le geste.
   */
  toggles: ReadonlyMap<string, number>;
  /** Pose le favori de la page s'il n'y est pas, le retire sinon ; tenue une fois la réponse là. */
  toggle(pageId: string): Promise<void>;
  /** Le refus du dernier geste, tel que le serveur le dit ; `null` : aucun. */
  error: string | null;
};

/** Ce que dit une réponse perdue quand la liste relue montre que rien n'a changé. */
const NOT_DONE = "le serveur n'a pas répondu, et le favori n'a pas changé. Réessaie.";

/** Aucune page : toujours le même tableau. */
const NONE: PageNode[] = [];

/**
 * Les pages de l'arbre `nodes` (`null` : pas encore lu) que nomment `ids`, dans leur ordre ; un
 * identifiant que l'arbre n'a pas n'est pas montré.
 */
export function favoritePages(ids: string[], nodes: PageNode[] | null): PageNode[] {
  if (nodes === null || ids.length === 0) return NONE;
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const pages = ids.flatMap((id) => byId.get(id) ?? []);
  return pages.length === 0 ? NONE : pages;
}

/** Un geste d'ici : ce qu'il demande, son rang, et l'écriture qui l'a confirmé. */
type Gesture = {
  on: boolean;
  /** Son rang parmi les gestes : seul le dernier sur une page la décide. */
  rank: number;
  /** Le rang de l'écriture qui l'a confirmé ; `null` : pas encore répondu. */
  confirmed: number | null;
};

type State = {
  spaceId: string | null;
  /** La dernière liste lue ; `null` : aucune encore. */
  read: string[] | null;
  /** Les gestes d'ici pas encore relus, par page. */
  gestures: ReadonlyMap<string, Gesture>;
  /** Le rang du dernier geste d'ici, par page, depuis qu'on montre cet espace. */
  toggles: ReadonlyMap<string, number>;
  error: string | null;
};

const empty = (spaceId: string | null): State => ({
  spaceId,
  read: null,
  gestures: new Map(),
  toggles: new Map(),
  error: null,
});

/** La liste lue, avec les gestes d'ici : retirés, ou ajoutés à la fin dans leur ordre. */
function withGestures(read: string[] | null, gestures: ReadonlyMap<string, Gesture>): string[] {
  const base = read ?? [];
  if (gestures.size === 0) return base;
  const kept = base.filter((id) => gestures.get(id)?.on !== false);
  const added = [...gestures]
    .filter(([id, gesture]) => gesture.on && !base.includes(id))
    .sort(([, a], [, b]) => a.rank - b.rank)
    .map(([id]) => id);
  return [...kept, ...added];
}

export function useFavorites(
  spaceId: string | null,
  onTreeRead: SpaceTree["onRead"],
  options: FavoritesOptions = {},
): FavoriteList {
  const [state, setState] = useState<State>(() => empty(spaceId));
  // Un autre espace : rien de l'espace d'avant n'est montré, dès ce rendu.
  const current = state.spaceId === spaceId ? state : empty(spaceId);
  if (state.spaceId !== spaceId) setState(current);

  // Les options du dernier rendu : en changer ne relance rien.
  const latest = useRef(options);
  latest.current = options;
  /** L'espace affiché, et ce qui est montré, à l'instant (pour `toggle`). */
  const shownSpace = useRef(spaceId);
  shownSpace.current = spaceId;
  const pages = useMemo(
    () => withGestures(current.read, current.gestures),
    [current.read, current.gestures],
  );
  const shown = useRef(pages);
  shown.current = pages;

  /** Demande une relecture (tout de suite). */
  const request = useRef<() => void>(() => {});
  /** Le rang de la dernière écriture confirmée : une lecture partie avant ne la reprend pas. */
  const writes = useRef(0);
  const ranks = useRef(0);
  /** Les gestes partent l'un après l'autre. */
  const chain = useRef<Promise<void>>(Promise.resolve());
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (spaceId === null) return;
    const rereads = scheduleRereads(async () => {
      const before = writes.current;
      try {
        const { pages: read } = await getFavorites(spaceId);
        if (rereads.stopped) return;
        setState((s) => {
          if (s.spaceId !== spaceId) return s;
          // Les gestes confirmés avant le départ de cette lecture y sont ; les autres restent.
          const gestures = new Map(s.gestures);
          for (const [id, gesture] of s.gestures) {
            if (gesture.confirmed !== null && gesture.confirmed <= before) gestures.delete(id);
          }
          return { ...s, read, gestures };
        });
      } catch (e) {
        if (rereads.stopped) return;
        // Session perdue : la connexion est demandée par-dessus l'application (`api.ts`), et la
        // liste est relue au retour du compte. Un autre échec garde ce qui est affiché.
        if (isSessionLost(e)) return SESSION_LOST;
      }
    }, waitForAccount(latest));
    request.current = () => rereads.request(true);
    // La première lecture, et les suivantes, attendent celle de l'arbre de l'espace.
    const off = onTreeRead((read) => {
      if (read === spaceId) rereads.request(true);
    });
    return () => {
      off();
      rereads.stop();
      request.current = () => {};
    };
  }, [spaceId, onTreeRead]);

  const toggle = useCallback((pageId: string): Promise<void> => {
    const space = shownSpace.current;
    if (space === null || !mounted.current) return Promise.resolve();
    const on = !shown.current.includes(pageId);
    const rank = ++ranks.current;
    setState((s) =>
      s.spaceId !== space
        ? s
        : {
            ...s,
            error: null,
            gestures: new Map(s.gestures).set(pageId, { on, rank, confirmed: null }),
            toggles: new Map(s.toggles).set(pageId, rank),
          },
    );
    const sent = chain.current.then(async () => {
      if (!mounted.current) return;
      let refused: unknown = null;
      try {
        await setFavorite(pageId, on);
      } catch (e) {
        refused = e;
      }
      if (answerLost(refused)) {
        // Le serveur a pu l'écrire : la liste le dit.
        try {
          const { pages: read } = await getFavorites(space);
          refused = read.includes(pageId) === on ? null : new ApiError(409, null, NOT_DONE);
        } catch {
          // La liste ne se lit pas : le doute reste, et se dit.
        }
      }
      if (!mounted.current || shownSpace.current !== space) return;
      const confirmed = refused === null ? ++writes.current : null;
      // Défait, le geste change ce qu'on montre, d'ici : un rang neuf, que la barre ne gèle pas.
      const undone = confirmed === null ? ++ranks.current : null;
      const told = refused !== null && !isSessionLost(refused);
      setState((s) => {
        if (s.spaceId !== space) return s;
        const gesture = s.gestures.get(pageId);
        // Un geste plus récent sur la même page la décide : celui-ci n'y change plus rien.
        if (gesture === undefined || gesture.rank !== rank) return s;
        const gestures = new Map(s.gestures);
        let toggles = s.toggles;
        if (undone !== null) {
          gestures.delete(pageId);
          toggles = new Map(s.toggles).set(pageId, undone);
        } else gestures.set(pageId, { ...gesture, confirmed: confirmed ?? 0 });
        return { ...s, gestures, toggles, error: told ? messageOf(refused) : s.error };
      });
      if (told) {
        const error =
          refused instanceof ApiError ? refused : new ApiError(0, null, messageOf(refused));
        latest.current.onRefused?.(error, pageId, on);
      }
      if (confirmed !== null) request.current();
    });
    chain.current = sent.catch(() => {});
    return sent;
  }, []);

  const { error, toggles } = current;
  const loaded = current.read !== null;
  return useMemo(
    () => ({ pages, loaded, toggles, toggle, error }),
    [pages, loaded, toggles, toggle, error],
  );
}
