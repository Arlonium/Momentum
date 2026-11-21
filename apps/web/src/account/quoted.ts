// Un texte venu d'ailleurs (le nom d'un jeton) cité dans une chaîne : un titre de `Confirm`, un nom
// accessible, là où un élément `dir` ne peut pas l'isoler. Les caractères d'isolement sont écrits par
// leur code, jamais en littéral ni en échappement : l'outil d'écriture changerait l'échappement en
// caractère réel, invisible à la relecture (R1623, piège de la Task C8).

/** U+2068, FIRST STRONG ISOLATE. */
export const FSI = String.fromCharCode(0x2068);
/** U+2069, POP DIRECTIONAL ISOLATE. */
export const PDI = String.fromCharCode(0x2069);
/** U+00A0, l'espace insécable des guillemets français. */
export const NBSP = String.fromCharCode(0xa0);

/** « nom », le nom isolé. */
export const quoted = (text: string) => `«${NBSP}${FSI}${text}${PDI}${NBSP}»`;
