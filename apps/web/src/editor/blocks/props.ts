// Lire une prop de bloc ou de contenu en ligne. Elle vient d'ailleurs (MMD, un agent, un autre
// client), et Yjs y garde toute valeur JSON : un nombre, un objet, une liste, que y-prosemirror
// passe telle quelle à l'éditeur. Rendue sans être vérifiée, elle cassait l'éditeur de tous ceux
// qui ouvrent la page (« Objects are not valid as a React child »). Toute prop de texte d'un bloc
// se lit donc ici, jamais par `block.props.<nom>` directement.

/** La prop `name` de `props` si c'est une chaîne, sinon `fallback`. */
export function textProp(props: Readonly<Record<string, unknown>>, name: string, fallback = ""): string {
  const value = props[name];
  return typeof value === "string" ? value : fallback;
}
