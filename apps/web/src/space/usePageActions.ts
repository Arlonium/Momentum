// Les actions sur les pages d'un espace, depuis la barre latérale et la page ouverte : créer,
// dupliquer, renommer, mettre à la corbeille, restaurer, déplacer ; et ce que dit un favori refusé
// (R254 : `useFavorites` le pose et le défait, le refus se dit ici). Le serveur fait foi. Une page
// ne change à l'écran qu'une fois le serveur d'accord ; sur son refus, rien n'a bougé, et son
// message s'affiche en pied de barre latérale (`notice`), jusqu'à la navigation suivante.
//
// Un refus dit que l'écran n'est plus celui du serveur (une page jetée, supprimée ou déplacée
// ailleurs) : la barre latérale est relue, et la page ouverte redemandée si l'action la
// visait. Sans cela, la ligne d'une page qui n'existe plus resterait, et chaque essai
// redonnerait la même erreur. Une session perdue n'est pas un refus : la connexion est
// demandée par-dessus l'application, sans message de plus.
//
// Ce que d'autres changent à la page ouverte arrive aussi par ici : elle est relue avant
// chaque connexion de synchro (`usePageGate`), que le serveur ferme quand ses droits changent.
// Niveau changé, page jetée ou restaurée (`reread`), page perdue (`lost`) : la page ouverte et
// sa ligne le savent aussitôt, et la barre latérale est relue pour ses sous-pages.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ApiError,
  createPage,
  duplicatePage,
  isSessionLost,
  restorePage,
  trashPage,
  updatePage,
} from "../api";
import type { OpenPage } from "../page/useOpenPage";
import { navigate, pagePath } from "../router";
import { messageOf } from "../session/forms";
import type { Drop } from "../sidebar/drag";
import { withoutSubtree } from "../sidebar/rows";
import type { CreateTarget } from "../sidebar/SectionList";
import type { SpaceTree } from "./useSpaceTree";

/** Un message du serveur, en phrase : son initiale en capitale. */
function capitalize(message: string): string {
  return message.charAt(0).toUpperCase() + message.slice(1);
}

/** Le message de l'erreur, en phrase. */
function sentence(error: unknown): string {
  return capitalize(messageOf(error));
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

/**
 * La personne écrit-elle quelque part (un champ, un texte modifiable : le titre, l'éditeur, un
 * commentaire) ? Une copie prête à ce moment-là ne s'ouvre pas : sa frappe irait dans le titre de
 * la copie, qui prend le focus (R646).
 */
function typing(): boolean {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement)) return false;
  return (
    active.isContentEditable ||
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement ||
    active instanceof HTMLSelectElement
  );
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
  /**
   * Duplique la page et ses sous-pages (R252), et ouvre la copie là où le serveur l'a mise : sous
   * la page parente, ou dans « Privé ». Pas si la personne est partie ailleurs, ou écrit, pendant
   * la requête (R646) : la copie paraît alors dans la barre latérale seulement.
   */
  duplicate: (id: string) => Promise<void>;
  /** Une duplication est en route : les entrées « Dupliquer » sont inactives (R605). */
  duplicating: boolean;
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
  /**
   * Ce que le serveur dit de la page ouverte, relue avant une connexion de synchro. Garde son
   * identité.
   */
  reread: (node: PageNode) => void;
  /**
   * Le serveur dit que la page n'est plus là pour ce compte (supprimée, ou plus partagée avec
   * lui). Garde son identité.
   */
  lost: (id: string) => void;
  /** Les accès de la page ouverte ont changé d'ici (menu Partager) : sa section a pu changer. */
  shared: () => void;
  /**
   * Le serveur a refusé un changement d'accès parce que la personne n'a plus l'accès complet :
   * le menu Partager s'est fermé, et son message se dit ici.
   */
  shareRefused: (message: string) => void;
  /**
   * Le serveur a refusé de poser (`on`) ou de retirer un favori : son message se dit ici, et la
   * barre latérale est relue (la page n'est plus visible, ou n'existe plus). Garde son identité.
   */
  favoriteRefused: (error: unknown, pageId: string, on: boolean) => void;
  /**
   * Un favori est posé ou retiré d'ici : comme toute action, il efface le refus d'avant (R468).
   * Garde son identité.
   */
  favoriteStarted: () => void;
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
  /** La page ouverte, telle qu'elle est à l'écran. */
  const shown = useRef(page.node);
  shown.current = page.node;
  const { change, touch, reload, latest } = tree;
  const { show, update, ask, gone } = page;
  const [trashChanges, setTrashChanges] = useState(0);

  /** Une création est en route : « Nouvelle page » et les « + » n'en lancent pas d'autre. */
  const creating = useRef(false);
  const create = useCallback(
    async (target: CreateTarget) => {
      // Deux demandes qui se suivent de près (un réseau lent, deux boutons) : la seconde
      // attend que la première ait répondu. Le second clic d'un double clic, lui, est ignoré
      // par les boutons (`firstClick`) : le serveur a souvent répondu avant qu'il n'arrive.
      if (isStale() || creating.current) return;
      creating.current = true;
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
      } finally {
        creating.current = false;
      }
    },
    [spaceId, isStale, change, reload],
  );

  /**
   * Une duplication est en route : « Dupliquer » n'en lance pas d'autre (le serveur la refuserait,
   * `429`, R605). L'état dit aux menus de griser leur entrée ; la référence garde les appels qui se
   * suivent dans le même rendu.
   */
  const duplicatingNow = useRef(false);
  const [duplicating, setDuplicating] = useState(false);
  const duplicate = useCallback(
    async (id: string) => {
      if (isStale() || duplicatingNow.current) return;
      duplicatingNow.current = true;
      setDuplicating(true);
      setNotice(null);
      // L'adresse au départ : partie ailleurs pendant la requête, la personne y reste (R646).
      const from = location.pathname;
      try {
        const copy = await duplicatePage(id);
        // Une relecture a pu l'apporter avant cette réponse : une seule ligne.
        change((nodes, space) =>
          space !== copy.spaceId || nodes.some((n) => n.id === copy.id) ? nodes : [...nodes, copy],
        );
        // Ses sous-pages ne sont pas dans la réponse : l'arbre est relu (le canal de l'espace le
        // ferait aussi, mais il peut être tombé).
        reload();
        if (location.pathname !== from || typing()) return;
        setCreated(copy.id);
        navigate(pagePath(copy.id));
      } catch (error) {
        if (isSessionLost(error)) return;
        setNotice(`La page n'a pas pu être dupliquée. ${sentence(error)}`);
        // `429` : le compte duplique déjà, ou a dépensé son budget ; le message dit quand revenir,
        // et rien ne le rejoue (R612). Il ne dit rien de la page : l'arbre n'a pas à être relu. Tout
        // autre refus dit que l'écran n'est plus celui du serveur.
        if (!(error instanceof ApiError && error.code === "duplication_busy")) reload();
      } finally {
        duplicatingNow.current = false;
        setDuplicating(false);
      }
    },
    [isStale, change, reload],
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

  const reread = useCallback(
    (node: PageNode) => {
      const known = shown.current;
      // La réponse d'une page qu'on a quittée entre-temps ne touche à rien.
      if (known === undefined || known.id !== node.id) return;
      const trashed = node.trashedAt !== null;
      // Ce qui change ce que la personne peut faire de la page, ou l'endroit où elle la voit :
      // ses sous-pages suivent, et l'arbre est à relire.
      const moved =
        known.level !== node.level ||
        (known.trashedAt !== null) !== trashed ||
        known.section !== node.section ||
        known.parentId !== node.parentId;
      if (!moved && known.title === node.title && known.position === node.position) return;
      if (trashed) {
        // À la corbeille, elle reste à l'écran et quitte l'arbre, avec ses sous-pages.
        show(node);
        change((nodes) => withoutSubtree(nodes, node.id));
      } else {
        update(node);
        change((nodes) => nodes.map((n) => (n.id === node.id ? node : n)));
      }
      if (!moved) return;
      // L'arbre est relu ici : inutile de le relire encore pour une page qui y revient.
      if (!trashed) caughtUp(node.id);
      reload();
    },
    [show, update, change, reload, caughtUp],
  );

  const lost = useCallback(
    (id: string) => {
      gone(id);
      change((nodes) => withoutSubtree(nodes, id));
      // Ce que le compte voit de l'espace a changé : l'arbre tel que le serveur le rend.
      reload();
    },
    [gone, change, reload],
  );

  const shared = useCallback(() => {
    touch();
    reload();
  }, [touch, reload]);

  const shareRefused = useCallback((message: string) => {
    setNotice(`Le partage n'a pas pu être modifié. ${capitalize(message)}`);
  }, []);

  const favoriteRefused = useCallback(
    (error: unknown, _pageId: string, on: boolean) => {
      setNotice(`Le favori n'a pas pu être ${on ? "ajouté" : "retiré"}. ${sentence(error)}`);
      reload();
    },
    [reload],
  );

  const favoriteStarted = useCallback(() => setNotice(null), []);

  return {
    notice,
    created,
    titleFocused,
    create,
    duplicate,
    duplicating,
    saved,
    trash,
    move,
    restore,
    trashChanged,
    trashChanges,
    reread,
    lost,
    shared,
    shareRefused,
    favoriteRefused,
    favoriteStarted,
  };
}
