// La page ouverte (`/p/:id`). Une page de l'arbre chargé se montre telle qu'il la porte, sans
// appel de plus. Sinon (lien direct, page d'un autre espace, page à la corbeille, page créée
// ailleurs depuis), le serveur est interrogé (`getPage`) : il dit si la personne la voit, dans
// quel espace, et depuis quand elle est à la corbeille. Une page qu'elle ne voit pas répond
// `404`, comme une page qui n'existe pas.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ApiError, getPage, isSessionLost } from "../api";
import { messageOf } from "../session/forms";

/** Ce que le serveur dit d'une page ouverte qui n'est pas dans l'arbre chargé. */
export type Lookup =
  | { id: string; state: "loading" }
  | { id: string; state: "found"; node: PageNode }
  /** Elle n'existe pas, ou la personne ne la voit pas : la même réponse. */
  | { id: string; state: "missing" }
  | { id: string; state: "failed"; message: string };

export type OpenPage = {
  /** Ce que le serveur dit de la page ouverte ; `null` : il n'a pas été interrogé pour elle. */
  looked: Lookup | null;
  /** La page ouverte : celle de l'arbre chargé, sinon celle que le serveur a rendue. */
  node: PageNode | undefined;
  /** Le serveur n'a pas encore dit où est la page ouverte : on ne sait pas quel espace montrer. */
  resolving: boolean;
  /** La page telle qu'on vient de l'apprendre (jetée ici) : elle reste à l'écran. */
  show(node: PageNode): void;
  /** La page a changé (renommée, restaurée) : si le serveur a été interrogé pour elle, la voici. */
  update(node: PageNode): void;
};

type Reading = {
  /** Change quand une lecture perdue avec la session peut être refaite. */
  retries: number;
  /** La lecture a répondu `401 unauthenticated`. */
  onSessionLost: () => void;
};

/**
 * `activeId` : la page que nomme l'adresse (`null` : aucune) ; `inTree` : elle-même, si
 * l'arbre chargé la porte.
 */
export function useOpenPage(
  activeId: string | null,
  inTree: PageNode | undefined,
  reading: Reading,
): OpenPage {
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const { retries } = reading;
  const callbacks = useRef(reading);
  callbacks.current = reading;

  const outsideTree = activeId !== null && inTree === undefined;
  useEffect(() => {
    if (activeId === null || !outsideTree) return;
    let cancelled = false;
    // Déjà rendue par le serveur : elle reste à l'écran pendant qu'il est relu.
    setLookup((current) =>
      current !== null && current.id === activeId && current.state === "found"
        ? current
        : { id: activeId, state: "loading" },
    );
    getPage(activeId).then(
      (node) => {
        if (!cancelled) setLookup({ id: activeId, state: "found", node });
      },
      (error: unknown) => {
        if (cancelled) return;
        if (isSessionLost(error)) callbacks.current.onSessionLost();
        else if (error instanceof ApiError && error.status === 404) {
          setLookup({ id: activeId, state: "missing" });
        } else setLookup({ id: activeId, state: "failed", message: messageOf(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [activeId, outsideTree, retries]);

  const show = useCallback((node: PageNode) => {
    setLookup({ id: node.id, state: "found", node });
  }, []);
  const update = useCallback((node: PageNode) => {
    setLookup((current) =>
      current !== null && current.id === node.id ? { id: node.id, state: "found", node } : current,
    );
  }, []);

  const looked = lookup !== null && lookup.id === activeId ? lookup : null;
  const node = inTree ?? (looked?.state === "found" ? looked.node : undefined);
  const resolving =
    activeId !== null && node === undefined && (looked === null || looked.state === "loading");
  return useMemo(
    () => ({ looked, node, resolving, show, update }),
    [looked, node, resolving, show, update],
  );
}
