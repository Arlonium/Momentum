// Les lignes de la fenêtre de recherche (lot 4, Task C1) : une par page, jamais deux.
//
// Deux sources. L'arbre de l'espace, déjà dans l'onglet et déjà filtré par les droits : les pages
// récentes pour un champ vide, sinon celles dont le titre contient ce qui est tapé (`findPages`),
// montrées tout de suite, sans réseau. Puis le serveur : les pages trouvées par leur titre ou leur
// texte, avec un extrait.
//
// Une ligne montrée ne bouge plus (R296, R572) : les lignes de l'arbre gardent leur rang et leur
// forme, celles du serveur s'ajoutent dessous, dans son ordre. Une page trouvée des deux côtés
// garde donc sa ligne de l'arbre, sans extrait : lui en donner un changerait sa hauteur, et
// déplacerait sous le pointeur toutes les lignes qui la suivent.
import type { PageNode, SearchHit } from "@mementum/protocol";

/** Une page au-dessus d'une page trouvée : du texte venu d'ailleurs. */
export type Ancestor = { id: string; title: string };

/** Une ligne : une page, son fil d'Ariane, et, trouvée par son texte, l'extrait et le bloc. */
export type Row = {
  /** L'identifiant de la page : la ligne active est tenue par lui, jamais par son rang. */
  id: string;
  title: string;
  /** Les pages au-dessus d'elle que le compte voit, de la plus haute à la page parente. */
  path: readonly Ancestor[];
  /**
   * Le bloc trouvé (Task C2 : y arriver) : un texte écrit par un client, à n'utiliser qu'échappé ;
   * `null` pour une page trouvée par son titre.
   */
  blockId: string | null;
  /** L'extrait du bloc trouvé, du texte, jamais du HTML ; vide pour une page trouvée par son titre. */
  excerpt: string;
  /** Les plages de `excerpt` à surligner, telles que le serveur les donne (`markedParts` les juge). */
  marks: readonly (readonly [number, number])[];
  /** D'où vient la ligne : l'arbre de l'onglet, ou la réponse du serveur. */
  from: "tree" | "server";
};

/**
 * Les lignes des pages `pages` de l'arbre, dans leur ordre, chacune avec son fil d'Ariane lu dans
 * l'arbre `nodes` (celui que le compte voit ; `null` : pas encore lu, aucun fil).
 */
export function treeRows(
  pages: readonly { id: string; title: string }[],
  nodes: readonly PageNode[] | null,
): Row[] {
  if (pages.length === 0) return [];
  const byId = new Map((nodes ?? []).map((node) => [node.id, node]));
  return pages.map((page) => {
    // Une chaîne qui boucle (que le serveur ne rend jamais) s'arrête à la première page revue.
    const seen = new Set([page.id]);
    const path: Ancestor[] = [];
    let parentId = byId.get(page.id)?.parentId ?? null;
    while (parentId !== null && !seen.has(parentId)) {
      const parent = byId.get(parentId);
      if (parent === undefined) break;
      seen.add(parentId);
      path.unshift({ id: parent.id, title: parent.title });
      parentId = parent.parentId;
    }
    return { id: page.id, title: page.title, path, blockId: null, excerpt: "", marks: [], from: "tree" };
  });
}

/**
 * `rows`, puis les pages de `hits` qui n'y sont pas encore, dans l'ordre du serveur. Les lignes de
 * `rows` sont rendues telles quelles, au même rang.
 */
export function withHits(rows: readonly Row[], hits: readonly SearchHit[]): Row[] {
  if (hits.length === 0) return rows as Row[];
  const shown = new Set(rows.map((row) => row.id));
  const merged = [...rows];
  for (const hit of hits) {
    if (shown.has(hit.pageId)) continue;
    shown.add(hit.pageId);
    merged.push({
      id: hit.pageId,
      title: hit.title,
      path: hit.path,
      blockId: hit.blockId,
      excerpt: hit.excerpt,
      marks: hit.marks,
      from: "server",
    });
  }
  return merged;
}
