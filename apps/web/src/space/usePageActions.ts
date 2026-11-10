// Les actions sur les pages d'un espace, depuis la barre latérale et la page ouverte : créer,
// renommer, mettre à la corbeille, restaurer, déplacer. Le serveur fait foi. Une page ne
// change à l'écran qu'une fois le serveur d'accord ; sur son refus, rien n'a bougé, et son
// message s'affiche en pied de barre latérale (`notice`), jusqu'à la navigation suivante.
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

/**
 * Le refus d'un déplacement, en phrase. Deux codes du serveur (`invalid_after`,
 * `invalid_parent`) parlent des champs de l'API, pas à la personne : on dit à leur place ce
 * qui s'est passé. Ils arrivent quand l'arbre a changé ailleurs entre-temps, ou dans
 * « Partagé », où deux pages voisines à l'écran peuvent être rangées sous des pages
 * différentes, que la personne ne voit pas.
 */
function moveRefusal(error: unknown): string {
  if (
    error instanceof ApiError &&
    (error.code === "invalid_after" || error.code === "invalid_parent")
  ) {
    return "Ces pages ne sont pas rangées ensemble dans l'espace, ou l'endroit visé a changé entre-temps.";
  }
  return sentence(error);
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
  /** Une page est sortie de la corbeille (depuis la vue de la corbeille). */
  restored: () => void;
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
  const { show, update } = page;

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
        change((nodes, space) => (space === node.spaceId ? [...nodes, node] : nodes));
        setCreated(node.id);
        navigate(pagePath(node.id));
      } catch (error) {
        setNotice(`La page n'a pas pu être créée. ${sentence(error)}`);
      }
    },
    [spaceId, isStale, change],
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
        // Session perdue : la connexion est demandée par-dessus, cela suffit.
        if (isSessionLost(error)) return;
        setNotice(`La page n'a pas pu être mise à la corbeille. ${sentence(error)}`);
      }
    },
    [latest, show, change],
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
        // Session perdue : la connexion est demandée par-dessus, cela suffit.
        if (isSessionLost(error)) return;
        // La page n'a pas bougé. Ce qui a fait refuser a pu changer ailleurs : la barre
        // latérale est relue, telle que le serveur la voit.
        setNotice(`La page n'a pas pu être déplacée. ${moveRefusal(error)}`);
        reload();
      }
    },
    [latest, change, reload],
  );

  const restored = useCallback(() => {
    touch();
    reload();
  }, [touch, reload]);

  const restore = useCallback(
    async (id: string) => {
      const node = await restorePage(id);
      update(node);
      // L'arbre est relu ici : pas besoin de le relire pour cette page absente.
      caughtUp(id);
      restored();
    },
    [update, caughtUp, restored],
  );

  return { notice, created, titleFocused, create, saved, trash, move, restore, restored };
}
