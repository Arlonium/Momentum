// La page ouverte (`/p/:id`). Une page de l'arbre chargé se montre telle qu'il la porte, sans
// appel de plus. Sinon (lien direct, page d'un autre espace, page à la corbeille, page créée
// ailleurs depuis), le serveur est interrogé (`getPage`) : il dit si la personne la voit, dans
// quel espace, et depuis quand elle est à la corbeille. Une page qu'elle ne voit pas répond
// `404`, comme une page qui n'existe pas.
//
// Une page affichée ne quitte l'écran que si le serveur dit qu'elle n'est plus là. Sortie de
// l'arbre chargé (une relecture ne la porte plus), elle reste telle qu'on la connaissait, à
// chaque rendu, le temps de sa réponse : un seul rendu sans elle démonterait son éditeur et
// recréerait sa session de synchro. Et s'il ne répond pas, elle reste encore, avec ce qu'on y
// a tapé. Une page que le serveur a dite introuvable est oubliée : revenir à son adresse la
// redemande, sans la remontrer d'abord.
//
// Une page ouverte peut aussi se perdre en route : avant chaque connexion de synchro, elle est
// relue (`usePageGate`), et le serveur peut répondre qu'elle n'est plus là pour ce compte
// (supprimée, ou plus partagée avec lui). L'écran le dit alors (`gone`), autrement que pour une
// adresse qui n'a jamais rien montré ; si elle revient dans l'arbre, elle se rouvre.
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
  /** Elle était ouverte, et le serveur dit qu'elle n'est plus là pour cette personne. */
  | { id: string; state: "gone" }
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
  /** Redemande au serveur la page ouverte, si elle n'est pas dans l'arbre chargé. */
  ask(): void;
  /**
   * Le serveur dit que la page ouverte `id` n'est plus là pour cette personne : elle quitte
   * l'écran, qui le dit. Sans effet si une autre page est ouverte.
   */
  gone(id: string): void;
};

type Reading = {
  /** Change quand une lecture perdue avec la session peut être refaite. */
  retries: number;
  /** La lecture a répondu `401 unauthenticated`. */
  onSessionLost: () => void;
};

/**
 * `activeId` : la page que nomme l'adresse (`null` : aucune) ; `loaded` : elle-même, si
 * l'arbre chargé la porte.
 */
export function useOpenPage(
  activeId: string | null,
  loaded: PageNode | undefined,
  reading: Reading,
): OpenPage {
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const { retries } = reading;
  const callbacks = useRef(reading);
  callbacks.current = reading;
  const active = useRef(activeId);
  active.current = activeId;
  /**
   * La page de l'arbre chargé que le serveur a dite perdue (`gone`) : tant que l'arbre la porte
   * encore telle quelle, elle ne compte plus. Une relecture qui la rapporte la rend.
   */
  const dead = useRef<PageNode | null>(null);
  const inTree = loaded === dead.current ? undefined : loaded;
  /**
   * La page ouverte, telle que l'arbre chargé la portait la dernière fois ; `null` : il ne l'a
   * pas portée, ou le serveur a dit depuis qu'elle n'est plus là.
   */
  const last = useRef<PageNode | null>(null);
  if (inTree !== undefined) last.current = inTree;
  /** Compte les demandes de relecture (`ask`). */
  const [asks, setAsks] = useState(0);

  // Dans l'arbre chargé, ce que le serveur avait répondu d'autre pour elle (introuvable,
  // injoignable) ne vaut plus : gardé, il la ferait disparaître le temps d'un rendu, le jour
  // où elle quitte l'arbre.
  if (inTree !== undefined && lookup !== null && lookup.id === inTree.id && lookup.state !== "found") {
    setLookup(null);
  }

  const outsideTree = activeId !== null && inTree === undefined;
  useEffect(() => {
    if (activeId === null || !outsideTree) return;
    let cancelled = false;
    // Déjà connue (le serveur l'a rendue, ou l'arbre la portait) : elle reste à l'écran
    // pendant que le serveur est relu.
    const known = last.current !== null && last.current.id === activeId ? last.current : null;
    setLookup((current) => {
      // Affichée, ou tout juste perdue : l'écran ne change pas pendant que le serveur est relu.
      const settled = current !== null && current.id === activeId;
      if (settled && (current.state === "found" || current.state === "gone")) return current;
      return known === null
        ? { id: activeId, state: "loading" }
        : { id: activeId, state: "found", node: known };
    });
    getPage(activeId).then(
      (node) => {
        if (!cancelled) setLookup({ id: activeId, state: "found", node });
      },
      (error: unknown) => {
        if (cancelled) return;
        if (isSessionLost(error)) callbacks.current.onSessionLost();
        else if (error instanceof ApiError && error.status === 404) {
          // Elle n'est plus là : ce qu'on en savait ne se remontre pas, même en y revenant.
          if (last.current !== null && last.current.id === activeId) last.current = null;
          // Perdue pendant qu'elle était ouverte : l'écran continue de le dire.
          setLookup((current) =>
            current !== null && current.id === activeId && current.state === "gone"
              ? current
              : { id: activeId, state: "missing" },
          );
        } else {
          // Le serveur ne répond pas : une page affichée le reste ; sinon, l'échec est dit.
          setLookup((current) =>
            current !== null && current.id === activeId && current.state === "found"
              ? current
              : { id: activeId, state: "failed", message: messageOf(error) },
          );
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [activeId, outsideTree, retries, asks]);

  const show = useCallback((node: PageNode) => {
    setLookup({ id: node.id, state: "found", node });
  }, []);
  const update = useCallback((node: PageNode) => {
    setLookup((current) =>
      current !== null && current.id === node.id ? { id: node.id, state: "found", node } : current,
    );
  }, []);

  const ask = useCallback(() => setAsks((n) => n + 1), []);

  const gone = useCallback((id: string) => {
    if (active.current !== id) return;
    if (last.current !== null && last.current.id === id) {
      dead.current = last.current;
      last.current = null;
    }
    setLookup({ id, state: "gone" });
  }, []);

  const looked = lookup !== null && lookup.id === activeId ? lookup : null;
  // Sortie de l'arbre chargé à ce rendu même : l'effet qui la redemande n'a pas encore tourné,
  // et elle reste telle que l'arbre la portait.
  const known = last.current !== null && last.current.id === activeId ? last.current : undefined;
  const fromServer = looked === null ? known : looked.state === "found" ? looked.node : undefined;
  const node = inTree ?? fromServer;
  const resolving =
    activeId !== null && node === undefined && (looked === null || looked.state === "loading");
  return useMemo(
    () => ({ looked, node, resolving, show, update, ask, gone }),
    [looked, node, resolving, show, update, ask, gone],
  );
}
