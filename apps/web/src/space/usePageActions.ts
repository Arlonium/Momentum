// Les actions sur les pages d'un espace, depuis la barre latérale et la page ouverte : créer,
// renommer, mettre à la corbeille, restaurer, déplacer. Le serveur fait foi. Une page ne
// change à l'écran qu'une fois le serveur d'accord ; sur son refus, rien n'a bougé, et son
// message s'affiche en pied de barre latérale (`notice`), jusqu'à la navigation suivante.
//
// Un refus dit que l'écran n'est plus celui du serveur (une page jetée, supprimée ou déplacée
// ailleurs) : la barre latérale est relue, et la page ouverte redemandée si l'action la
// visait. Sans cela, la ligne d'une page qui n'existe plus resterait, et chaque essai
// redonnerait la même erreur. Une session perdue n'est pas un refus : la connexion est
// demandée par-dessus l'application, sans message de plus.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, createPage, isSessionLost, restorePage, trashPage, updatePage } from "../api";
import type { OpenPage } from "../page/useOpenPage";
import { navigate, pagePath } from "../router";
import { messageOf } from "../session/forms";
import type { Drop } from "../sidebar/drag";
import { withoutSubtree } from "../sidebar/rows";
import type { CreateTarget } from "../sidebar/SectionList";
import type { SpaceTree } from "./useSpaceTree";

/** Le message du serveur, en phrase : son initiale en capitale. */
function sentence(error: unknown): string {
  const message = messageOf(error);
  return message.charAt(0).toUpperCase() + message.slice(1);
}

/** Le serveur dit-il que l'endroit visé n'existe pas, dans les mots de l'API ? */
function isPlacement(error: unknown): boolean {
  return (
    error instanceof ApiError &&
    (error.code === "invalid_after" || error.code === "invalid_parent")
  );
}

/**
 * Le refus d'un déplacement, en phrase. Deux codes du serveur (`invalid_after`,
 * `invalid_parent`) parlent des champs de l'API, pas à la personne : on dit à leur place ce
 * qui s'est passé. Ils arrivent quand l'arbre a changé ailleurs entre-temps, ou dans
 * « Partagé », où deux pages voisines à l'écran peuvent être rangées sous des pages
 * différentes, que la personne ne voit pas.
 */
function moveRefusal(error: unknown): string {
  return isPlacement(error)
    ? "Ces pages ne sont pas rangées ensemble dans l'espace, ou l'endroit visé a changé entre-temps."
    : sentence(error);
}

/** Le refus d'une création, en phrase : les mêmes codes disent que la page parente a disparu. */
function creationRefusal(error: unknown): string {
  return isPlacement(error)
    ? "Sa page parente n'est plus là, ou tu ne la vois plus."
    : sentence(error);
}

type Inputs = {
  tree: SpaceTree;
  page: OpenPage;
  /** L'espace affiché : celui où une page se crée à la racine. */
  spaceId: string | null;
  /** La page que nomme l'adresse, sinon `null`. */
  activeId: string | null;
  /** L'adresse : le refus affiché part quand elle change. */
  pathname: string;
  /** Le serveur n'a pas le schéma de ce client : aucune création. */
  isStale: () => boolean;
  /** L'arbre vient d'être relu pour cette page (`useTreeCatchUp`). */
  caughtUp: (id: string) => void;
};

export type PageActions = {
  /** Le refus de la dernière action, en français ; `null` : aucun. */
  notice: string | null;
  /** Page tout juste créée : son titre prendra le focus, à sa première ouverture seulement. */
  created: string | null;
  titleFocused: () => void;
  /** Crée une page (sous une page, ou à la racine d'une section) et l'ouvre. */
  create: (target: CreateTarget) => Promise<void>;
  /** Le titre d'une page a été enregistré. */
  saved: (node: PageNode) => void;
  /** Met la page à la corbeille, avec ses sous-pages. */
  trash: (id: string) => Promise<void>;
  /** Range la page ailleurs dans sa section. */
  move: (id: string, drop: Drop) => Promise<void>;
  /** Sort la page ouverte de la corbeille ; rejette avec l'erreur du serveur s'il refuse. */
  restore: (id: string) => Promise<void>;
  /**
   * La vue de la corbeille a restauré une page, ou le serveur lui a refusé une action : la
   * barre latérale est relue.
   */
  trashChanged: () => void;
  /** Change quand une page est jetée ou restaurée d'ici : la vue de la corbeille se relit. */
  trashChanges: number;
};

export function usePageActions({
  tree,
  page,
  spaceId,
  activeId,
  pathname,
  isStale,
  caughtUp,
}: Inputs): PageActions {
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => setNotice(null), [pathname]);
  const [created, setCreated] = useState<string | null>(null);
  const titleFocused = useCallback(() => setCreated(null), []);
  const open = useRef(activeId);
  open.current = activeId;
  const { change, touch, reload, latest } = tree;
  const { show, update, ask } = page;
  const [trashChanges, setTrashChanges] = useState(0);

  const create = useCallback(
    async (target: CreateTarget) => {
      if (isStale()) return;
      setNotice(null);
      try {
        // Sans `afterId` : en fin des frères (R23). À la racine, il faut dire l'espace et la
        // section ; une sous-page est dans l'espace de sa page parente, et hérite de ses accès.
        const node = await createPage(
          "parentId" in target ? target : { spaceId, section: target.section },
        );
        // Une relecture a pu l'apporter avant cette réponse : une seule ligne.
        change((nodes, space) =>
          space !== node.spaceId || nodes.some((n) => n.id === node.id) ? nodes : [...nodes, node],
        );
        setCreated(node.id);
        navigate(pagePath(node.id));
      } catch (error) {
        if (isSessionLost(error)) return;
        setNotice(`La page n'a pas pu être créée. ${creationRefusal(error)}`);
        reload();
      }
    },
    [spaceId, isStale, change, reload],
  );

  const saved = useCallback(
    (node: PageNode) => {
      change((nodes) => nodes.map((n) => (n.id === node.id ? node : n)));
      update(node);
    },
    [change, update],
  );

  const trash = useCallback(
    async (id: string) => {
      setNotice(null);
      try {
        const trashed = await trashPage(id);
        // La page ouverte part avec elle (elle-même, ou une de ses pages parentes) : elle
        // reste à l'écran, à la corbeille, le temps que le serveur le confirme.
        const before = latest()?.nodes ?? [];
        const shown = before.find((n) => n.id === open.current);
        if (shown !== undefined && !withoutSubtree(before, id).includes(shown)) {
          show(shown.id === id ? trashed : { ...shown, trashedAt: trashed.trashedAt });
        }
        change((nodes) => withoutSubtree(nodes, id));
      } catch (error) {
        if (isSessionLost(error)) return;
        // Jetée ou supprimée ailleurs, peut-être : la barre latérale est relue, et la page
        // ouverte avec elle (sortie de l'arbre, elle est redemandée au serveur).
        setNotice(`La page n'a pas pu être mise à la corbeille. ${sentence(error)}`);
        reload();
      }
      setTrashChanges((n) => n + 1);
    },
    [latest, show, change, reload],
  );

  const move = useCallback(
    async (id: string, drop: Drop) => {
      const node = latest()?.nodes.find((n) => n.id === id);
      if (node === undefined) return;
      setNotice(null);
      // Parmi les mêmes sœurs, la page garde sa page parente, qu'on ne nomme pas : celle que
      // le serveur lui connaît n'est pas toujours visible d'ici (`parentId` vaut alors `null`).
      const sameParent = drop.parentId === node.parentId;
      try {
        const moved = await updatePage(id, sameParent ? { afterId: drop.afterId } : drop);
        change((nodes) => nodes.map((n) => (n.id === id ? moved : n)));
        // Sous une autre page, la page et ses sous-pages héritent d'autres accès : l'arbre est
        // relu.
        if (!sameParent) reload();
      } catch (error) {
        if (isSessionLost(error)) return;
        // La page n'a pas bougé. Ce qui a fait refuser a pu changer ailleurs : la barre
        // latérale est relue, telle que le serveur la voit.
        setNotice(`La page n'a pas pu être déplacée. ${moveRefusal(error)}`);
        reload();
      }
    },
    [latest, change, reload],
  );

  const trashChanged = useCallback(() => {
    touch();
    reload();
  }, [touch, reload]);

  const restore = useCallback(
    async (id: string) => {
      try {
        const node = await restorePage(id);
        update(node);
        // L'arbre est relu ici : pas besoin de le relire pour cette page absente.
        caughtUp(id);
      } catch (error) {
        // Supprimée ou restaurée ailleurs, peut-être : la page est redemandée au serveur. Le
        // bandeau dit le refus.
        if (!isSessionLost(error)) ask();
        throw error;
      } finally {
        trashChanged();
        setTrashChanges((n) => n + 1);
      }
    },
    [update, caughtUp, ask, trashChanged],
  );

  return {
    notice,
    created,
    titleFocused,
    create,
    saved,
    trash,
    move,
    restore,
    trashChanged,
    trashChanges,
  };
}
