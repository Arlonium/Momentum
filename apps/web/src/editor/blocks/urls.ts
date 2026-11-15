// Les adresses qu'un bloc suit ou montre (plan de la partie B, « Global Constraints », « Client ») :
// une URL n'est suivie ou chargée que si son schéma est permis. Ici, une adresse web, `http:` ou
// `https:`, écrite en entier ; un chemin de fichier de l'instance s'y ajoutera avec les fichiers
// (Task C8, `isFilePath`). Une adresse vient d'ailleurs (MMD, un autre client, ce qu'on tape) :
// ce qui n'en est pas une est refusé, jamais complété (`exemple.fr` ne devient pas
// `https://exemple.fr/`).

/** Une adresse web s'écrit avec son schéma et ses deux barres (`http:exemple`, que l'URL admet, non). */
const WEB_PREFIX = /^https?:\/\//i;

/**
 * L'adresse `raw`, sous la forme qu'en écrit le navigateur (`https://mementum.dev` devient
 * `https://mementum.dev/`, un domaine hors de l'ASCII passe en punycode), si c'est une adresse
 * web entière ; sinon `null`. Tout ce qui n'est pas une chaîne (une prop écrite par un autre
 * client) est refusé.
 */
export function webUrl(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const trimmed = raw.trim();
  if (!WEB_PREFIX.test(trimmed)) return null;
  try {
    const url = new URL(trimmed);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

/** L'hôte d'une adresse web (avec son port s'il n'est pas celui par défaut), sinon `null`. */
export function hostOf(raw: unknown): string | null {
  const href = webUrl(raw);
  return href === null ? null : new URL(href).host;
}
