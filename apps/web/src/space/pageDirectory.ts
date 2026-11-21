// Les pages que le compte voit dans l'espace affiché, pour l'éditeur (Task C3, R251) : le titre à
// jour d'un lien de page (et des mentions, Task C4), et les pages qu'un sélecteur propose (R570).
//
// Elles viennent de l'arbre de l'espace, que le serveur a déjà filtré par les droits (`App` les
// publie à chaque arbre lu, `publishPages`). Une page qui n'y est pas (qu'on ne voit pas, d'un autre
// espace, à la corbeille, ou l'arbre pas encore lu) n'a pas de titre ici, et aucun sélecteur ne la
// propose : un lien vers elle ne montre que le libellé copié dans le document. Aucune lecture du
// serveur ne part d'ici.
//
// Un magasin du module, lu par `useSyncExternalStore` : l'éditeur n'est pas rendu de nouveau quand
// l'arbre change ; seule la vue d'un lien dont le titre a changé l'est (`usePageTitle`).
import type { PageNode } from "@mementum/protocol";
import { useEffect, useSyncExternalStore } from "react";

/** Une page proposée par un sélecteur : son id et son titre, rien d'autre. */
export type FoundPage = { id: string; title: string };

/** Une page à sa place dans l'arbre ([`outlinePages`]) : sa profondeur, 0 à la racine de ce qu'on voit. */
export type OutlinedPage = FoundPage & { depth: number };

type Directory = {
  readonly spaceId: string | null;
  /** Les pages, dans l'ordre de l'arbre. */
  readonly pages: readonly FoundPage[];
  /** La page parente visible de chaque page qui en a une, par id. */
  readonly parents: ReadonlyMap<string, string>;
  /** Le titre de chaque page, par id. */
  readonly titles: ReadonlyMap<string, string>;
  /** Le titre de chaque page, sans accents ni casse, dans l'ordre de `pages`. */
  readonly folded: readonly string[];
};

const EMPTY: Directory = { spaceId: null, pages: [], parents: new Map(), titles: new Map(), folded: [] };

let directory: Directory = EMPTY;
const listeners = new Set<() => void>();

/** Un texte sans accents ni casse, pour comparer une requête à un titre. */
function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase("fr");
}

function samePages(a: readonly FoundPage[], b: readonly FoundPage[]): boolean {
  return a.length === b.length && a.every((page, i) => page.id === b[i].id && page.title === b[i].title);
}

function sameParents(a: ReadonlyMap<string, string>, b: ReadonlyMap<string, string>): boolean {
  if (a.size !== b.size) return false;
  for (const [id, parent] of a) if (b.get(id) !== parent) return false;
  return true;
}

/** Les pages de la racine et les sous-pages de chaque page, bâties à la première demande. */
type Tree = { readonly roots: readonly FoundPage[]; readonly children: ReadonlyMap<string, readonly FoundPage[]> };

/** L'arbre des pages publiées ([`outlinePages`]), refait au prochain arbre publié. */
let tree: Tree | null = null;

function treeOfPages(): Tree {
  if (tree === null) {
    const { pages, parents } = directory;
    const known = new Set(pages.map((page) => page.id));
    const roots: FoundPage[] = [];
    const children = new Map<string, FoundPage[]>();
    for (const page of pages) {
      const parent = parents.get(page.id);
      if (parent === undefined || !known.has(parent)) {
        roots.push(page);
        continue;
      }
      const list = children.get(parent);
      if (list === undefined) children.set(parent, [page]);
      else list.push(page);
    }
    tree = { roots, children };
  }
  return tree;
}

/** Ce que montre une liste qui suit l'arbre ([`outlinePages`]) : ses pages, et combien elle en tait. */
export type Outline = { pages: OutlinedPage[]; hidden: number };

/**
 * Les pages publiées dans l'ordre où l'arbre les montre (recette du lot 5, B3, Rulings R1553, R1690) :
 * chaque page suivie de ses sous-pages, en profondeur, chacune avec sa profondeur. L'arbre que rend
 * le serveur est rangé par page parente (`pages` aussi) : une sous-page y vient après les pages de la
 * racine. Une page dont la page parente n'est pas publiée est à la racine de ce qu'on voit.
 *
 * Au plus `limit` pages, et **toutes les pages de la racine d'abord** : leurs sous-pages ne sont
 * dépliées que dans la place qui reste, dans l'ordre de l'arbre ; sans cela, la première page qui a
 * des sous-pages repoussait les autres pages de la racine hors de la liste. `hidden` dit combien de
 * pages la liste tait.
 */
export function outlinePages(limit: number): Outline {
  const { roots, children } = treeOfPages();
  const total = directory.pages.length;
  if (roots.length >= limit) {
    const pages = roots.slice(0, limit).map((page) => ({ ...page, depth: 0 }));
    return { pages, hidden: total - pages.length };
  }
  let room = limit - roots.length;
  const pages: OutlinedPage[] = [];
  const seen = new Set<string>();
  for (const root of roots) {
    pages.push({ ...root, depth: 0 });
    seen.add(root.id);
    const stack: OutlinedPage[] = [...(children.get(root.id) ?? [])].reverse().map((page) => ({ ...page, depth: 1 }));
    while (room > 0 && stack.length > 0) {
      const page = stack.pop()!;
      if (seen.has(page.id)) continue;
      seen.add(page.id);
      pages.push(page);
      room -= 1;
      const kids = children.get(page.id) ?? [];
      for (let i = kids.length - 1; i >= 0; i--) stack.push({ ...kids[i], depth: page.depth + 1 });
    }
  }
  return { pages, hidden: total - pages.length };
}

/**
 * Les pages de l'arbre de l'espace `spaceId` que voit le compte (`null` : pas encore lues, ou plus
 * d'espace affiché). Appelé par `App` à chaque arbre lu, et quand on quitte l'espace ou l'application.
 * Une page à la corbeille n'y entre pas.
 */
export function publishPages(spaceId: string | null, nodes: readonly PageNode[] | null): void {
  const kept = nodes === null ? [] : nodes.filter((node) => node.trashedAt === null);
  const pages = kept.map((node) => ({ id: node.id, title: node.title }));
  const parents = new Map<string, string>();
  for (const node of kept) if (node.parentId !== null) parents.set(node.id, node.parentId);
  if (spaceId === directory.spaceId && samePages(pages, directory.pages) && sameParents(parents, directory.parents)) {
    return;
  }
  directory =
    pages.length === 0 && spaceId === null
      ? EMPTY
      : {
          spaceId,
          pages,
          parents,
          titles: new Map(pages.map((page) => [page.id, page.title])),
          folded: pages.map((page) => fold(page.title)),
        };
  tree = null;
  for (const listener of [...listeners]) listener();
}

/**
 * Publie l'arbre de l'espace affiché tant que l'application est montée (`App`) : à chaque arbre lu,
 * rien tant que l'espace n'a pas le sien, et plus rien quand elle part (une session fermée, un autre
 * compte) : les titres d'un compte ne restent pas au suivant.
 */
export function usePublishedTree(spaceId: string | null, nodes: readonly PageNode[] | null): void {
  useEffect(() => publishPages(spaceId, nodes), [spaceId, nodes]);
  useEffect(() => () => publishPages(null, null), []);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Les pages que l'arbre affiché porte à cet instant, par id : une photo, qui ne change plus (chaque
 * arbre publié en fait une neuve). Pour reconnaître plus tard une page qui vient d'apparaître.
 */
export function publishedTitles(): ReadonlyMap<string, string> {
  return directory.titles;
}

/** L'espace dont l'arbre est publié (celui qu'affiche la barre latérale) ; `null` : aucun. */
export function publishedSpace(): string | null {
  return directory.spaceId;
}

/** Le titre à jour de la page `id` si le compte la voit dans l'espace affiché, sinon `null`. */
export function pageTitle(id: string): string | null {
  return directory.titles.get(id) ?? null;
}

/**
 * Le titre à jour de la page `id`, suivi : le composant n'est rendu de nouveau que quand ce titre
 * change (ou que la page entre dans l'arbre, ou en sort). `null` : le compte ne la voit pas.
 */
export function usePageTitle(id: string): string | null {
  return useSyncExternalStore(subscribe, () => pageTitle(id));
}

/**
 * `listener` à chaque arbre publié qui change les pages, jusqu'à l'appel de la fonction rendue : pour
 * la vue d'un lien de page dessinée sans React (R627), qui relit alors son titre (`pageTitle`).
 */
export function watchPages(listener: () => void): () => void {
  return subscribe(listener);
}

/** Combien d'abonnés suivent les pages (les tests : une vue partie ne suit plus rien). */
export function watchersForTests(): number {
  return listeners.size;
}

/** Les pages publiées, suivies : leur liste change quand l'arbre change (pour un sélecteur). */
export function usePublishedPages(): readonly FoundPage[] {
  return useSyncExternalStore(subscribe, () => directory.pages);
}

/**
 * Au plus `limit` pages publiées dont le titre contient `query`, sans accents ni casse, espaces de
 * bord retirées : celles dont le titre commence par la requête d'abord, puis les autres, chacune
 * dans l'ordre de l'arbre. Une requête vide : les premières pages de l'arbre.
 */
export function findPages(query: string, limit: number): FoundPage[] {
  const wanted = fold(query.trim());
  const { pages, folded } = directory;
  const first: FoundPage[] = [];
  const then: FoundPage[] = [];
  for (let i = 0; i < pages.length && first.length < limit; i++) {
    const title = folded[i];
    if (title.startsWith(wanted)) first.push(pages[i]);
    else if (then.length < limit && title.includes(wanted)) then.push(pages[i]);
  }
  return [...first, ...then].slice(0, limit);
}

/** Un identifiant de page : un UUID, rien d'autre (jamais un chemin, une adresse ou un texte). */
const PAGE_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isPageId(id: string): boolean {
  return PAGE_ID.test(id);
}
