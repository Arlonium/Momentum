// La pastille d'un compte : ses initiales, sur sa couleur de présence (la même que son
// curseur dans une page). Menu du compte, liste des membres, et bientôt le menu Partager.
import { colorOf } from "../session/session";

/** Les initiales d'un nom : celles des deux premiers mots, ou la première lettre. */
function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const first = (word: string) => Array.from(word)[0] ?? "";
  const letters = words.length >= 2 ? first(words[0]) + first(words[1]) : first(words[0] ?? "");
  return letters.toLocaleUpperCase("fr");
}

export function Avatar({ id, name, size }: { id: string; name: string; size: number }) {
  return (
    <span
      className="mm-avatar"
      aria-hidden="true"
      style={{ background: colorOf(id), width: size, height: size, fontSize: size <= 24 ? 10 : 11 }}
    >
      {initials(name)}
    </span>
  );
}
