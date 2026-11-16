// Un message bref d'un geste de l'éditeur refusé, posé sur la page, hors de l'arbre de React : un
// lien trop long (`linkBound.ts`, R495), une copie de trop de blocs (`duplicate.ts`, R641). Une zone
// d'alerte par sorte (sa classe) : redit, le message reste six secondes de plus ; un message retiré
// par un autre (un test) est reposé.

/** Durée d'affichage d'un message, en millisecondes. */
const NOTICE_MS = 6_000;

const notices = new Map<string, { element: HTMLElement; timer: ReturnType<typeof setTimeout> }>();

/** Montre `text` dans la zone d'alerte de la sorte `kind` (sa classe), six secondes. */
export function announce(kind: string, text: string): void {
  if (typeof document === "undefined") return;
  const shown = notices.get(kind);
  if (shown !== undefined && shown.element.isConnected) {
    clearTimeout(shown.timer);
    shown.element.textContent = text;
    shown.timer = setTimeout(() => dismissNotice(kind), NOTICE_MS);
    return;
  }
  if (shown !== undefined) clearTimeout(shown.timer);
  const element = document.createElement("div");
  element.className = kind;
  element.setAttribute("role", "alert");
  element.style.cssText =
    "position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:1000;" +
    "padding:8px 14px;border-radius:6px;background:#2f2e2b;color:#fff;font-size:14px;";
  element.textContent = text;
  document.body.append(element);
  notices.set(kind, { element, timer: setTimeout(() => dismissNotice(kind), NOTICE_MS) });
}

/** Retire le message de la sorte `kind`, s'il est là. */
export function dismissNotice(kind: string): void {
  const shown = notices.get(kind);
  if (shown === undefined) return;
  clearTimeout(shown.timer);
  shown.element.remove();
  notices.delete(kind);
}
