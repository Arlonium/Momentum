// L'initiale d'un avatar, et sa couleur. La palette de présence (`colorOf`) est celle des
// curseurs et ne change pas : c'est le texte qui s'adapte, pour que l'initiale tienne 4,5:1 sur
// chaque couleur (conception §4.3). Le blanc, sauf là où il n'y arrive pas : l'encre du thème.

/** `--mm-text-strong` de `theme.css`. */
export const INK = "#1f1e1c";
const WHITE = "#ffffff";

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((at) => {
    const value = parseInt(hex.slice(at, at + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

/** Rapport de contraste WCAG entre deux couleurs `#RRGGBB`. */
export function contrast(a: string, b: string): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (high + 0.05) / (low + 0.05);
}

/** Blanc quand il tient 4,5:1 sur `background`, sinon l'encre du thème. */
export function inkOn(background: string): string {
  return contrast(WHITE, background) >= 4.5 ? WHITE : INK;
}

/**
 * L'initiale d'un nom dans un avatar : sa première lettre ou son premier chiffre, en capitale,
 * jamais un caractère de format (U+202E, U+2068…) ni une espace ; « ? » s'il n'y en a pas. La
 * même partout : présents de la page, fils de commentaires, historique.
 */
export function initialOf(name: string): string {
  return name.match(/[\p{L}\p{N}]/u)?.[0]?.toLocaleUpperCase() ?? "?";
}
