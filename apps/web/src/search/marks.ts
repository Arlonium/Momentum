// Les mots trouvés d'un extrait (lot 4, Task C1). Le serveur ne rend jamais de HTML : le texte de
// l'extrait, et des plages `[début, fin[` à surligner, comptées en unités UTF-16 (les places d'une
// chaîne JavaScript), dans l'ordre et sans chevauchement (`SearchHit.marks`). Ici, l'extrait est
// coupé en morceaux de texte, surlignés ou non ; la fenêtre rend chacun en nœud de texte, un `<mark>`
// autour des surlignés. Rien de l'extrait ne passe jamais par du HTML.
//
// L'extrait vient du texte d'une page, écrit par n'importe qui, et les plages d'une réponse du
// réseau : une plage qui ne tient pas (hors du texte, vide, à l'envers, pas faite de places entières,
// qui chevauche celle d'avant, ou qui coupe une paire UTF-16 en deux) est ignorée, jamais corrigée.
// Le texte, lui, est toujours rendu en entier.

/** Un morceau de l'extrait, dans l'ordre : surligné ou non. */
export type Part = { text: string; marked: boolean };

/** La place `at` est-elle entre les deux unités d'une paire UTF-16 de `text` ? */
function inPair(text: string, at: number): boolean {
  if (at <= 0 || at >= text.length) return false;
  const before = text.charCodeAt(at - 1);
  const after = text.charCodeAt(at);
  return before >= 0xd800 && before <= 0xdbff && after >= 0xdc00 && after <= 0xdfff;
}

/**
 * `excerpt` coupé d'après `marks`. Aucun morceau n'est vide, et mis bout à bout ils redonnent
 * l'extrait, quelles que soient les plages.
 */
export function markedParts(excerpt: string, marks: readonly (readonly [number, number])[]): Part[] {
  const parts: Part[] = [];
  /** Jusqu'où l'extrait est déjà rendu. */
  let at = 0;
  for (const mark of Array.isArray(marks) ? marks : []) {
    if (!Array.isArray(mark) || mark.length !== 2) continue;
    const [start, end] = mark as [unknown, unknown];
    if (!Number.isInteger(start) || !Number.isInteger(end)) continue;
    const from = start as number;
    const to = end as number;
    if (from < at || to <= from || to > excerpt.length) continue;
    if (inPair(excerpt, from) || inPair(excerpt, to)) continue;
    if (from > at) parts.push({ text: excerpt.slice(at, from), marked: false });
    parts.push({ text: excerpt.slice(from, to), marked: true });
    at = to;
  }
  if (at < excerpt.length) parts.push({ text: excerpt.slice(at), marked: false });
  return parts;
}
