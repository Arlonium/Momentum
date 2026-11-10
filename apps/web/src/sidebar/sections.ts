// Les trois sections de la barre latérale d'un espace, comme dans Notion (conception, §4.4) :
// « Espace » (ce que voient tous les membres), « Partagé » (avec des personnes précises) et
// « Privé » (soi seul). Le serveur calcule la section de chaque page pour la personne
// connectée, d'après ses accès : rien ne se range à la main.
import type { PageNode, Role, Section } from "@mementum/protocol";

/** Les sections, dans l'ordre de la barre latérale. */
export const SECTIONS: readonly Section[] = ["space", "shared", "private"];

/** Le titre de chaque section. */
export const SECTION_LABELS: Record<Section, string> = {
  space: "Espace",
  shared: "Partagé",
  private: "Privé",
};

/**
 * Les pages de chaque section, dans l'ordre reçu. Une sous-page suit la section de sa plus
 * haute page visible (celle qui n'a pas de parent dans `nodes`) : un sous-arbre n'est jamais
 * coupé entre deux sections, même si une page porte une section plus ancienne que celle de
 * sa page parente.
 */
export function bySection(nodes: PageNode[]): Record<Section, PageNode[]> {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const known = new Map<string, Section>();
  const sectionOf = (node: PageNode): Section => {
    // On remonte jusqu'à une page déjà classée, ou jusqu'à la plus haute. Une page revue en
    // remontant (des parents en rond, que le serveur ne rend jamais) arrête la montée.
    const path = new Set<string>();
    let current = node;
    let section = known.get(current.id);
    while (section === undefined) {
      path.add(current.id);
      const parent = current.parentId === null ? undefined : byId.get(current.parentId);
      if (parent === undefined || path.has(parent.id)) {
        section = current.section;
      } else {
        current = parent;
        section = known.get(current.id);
      }
    }
    for (const id of path) known.set(id, section);
    return section;
  };
  const groups: Record<Section, PageNode[]> = { space: [], shared: [], private: [] };
  for (const node of nodes) groups[sectionOf(node)].push(node);
  return groups;
}

/**
 * Les sections à montrer, dans l'ordre. Un invité n'a que « Partagé » (Ruling R57). Un membre
 * a toujours « Espace » et « Privé », où il crée des pages, et « Partagé » quand on a partagé
 * quelque chose avec lui.
 */
export function visibleSections(role: Role, groups: Record<Section, PageNode[]>): Section[] {
  if (role === "guest") return ["shared"];
  return SECTIONS.filter((section) => section !== "shared" || groups.shared.length > 0);
}

/**
 * Crée-t-on une page à la racine de cette section ? « Partagé » ne se choisit pas : une page
 * y arrive quand on la partage.
 */
export function canCreateIn(section: Section): section is "space" | "private" {
  return section !== "shared";
}

const collapsedKey = (spaceId: string) => `mementum.sections.${spaceId}`;

/** Les sections repliées de cet espace, gardées dans le navigateur. */
export function readCollapsedSections(spaceId: string): Section[] {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(collapsedKey(spaceId)) ?? "[]");
    if (!Array.isArray(stored)) return [];
    return SECTIONS.filter((section) => stored.includes(section));
  } catch {
    // stockage refusé, ou valeur illisible : tout est déplié
    return [];
  }
}

/** Garde les sections repliées de cet espace, pour la prochaine visite. */
export function writeCollapsedSections(spaceId: string, collapsed: readonly Section[]): void {
  try {
    if (collapsed.length === 0) localStorage.removeItem(collapsedKey(spaceId));
    else localStorage.setItem(collapsedKey(spaceId), JSON.stringify(collapsed));
  } catch {
    // stockage refusé : l'état ne vaut que pour cette visite
  }
}
