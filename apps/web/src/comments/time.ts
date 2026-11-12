// L'âge d'un message de commentaire, « il y a… », écrit à la main en français : jusqu'à une
// semaine, en minutes, heures ou jours ; au-delà, sa date. La date complète va dans l'infobulle.

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const DAY_MONTH = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "long" });
const DAY_MONTH_YEAR = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "long",
  year: "numeric",
});
const FULL = new Intl.DateTimeFormat("fr-FR", { dateStyle: "long", timeStyle: "short" });

function ago(count: number, one: string, many: string): string {
  return `il y a ${count} ${count === 1 ? one : many}`;
}

/** L'âge de `at` à l'instant `now` (millisecondes Unix). Une date à venir vaut « à l'instant ». */
export function relativeTime(at: number, now: number): string {
  const elapsed = now - at;
  if (elapsed < MINUTE) return "à l'instant";
  if (elapsed < HOUR) return ago(Math.floor(elapsed / MINUTE), "minute", "minutes");
  if (elapsed < DAY) return ago(Math.floor(elapsed / HOUR), "heure", "heures");
  if (elapsed < 7 * DAY) return ago(Math.floor(elapsed / DAY), "jour", "jours");
  const date = new Date(at);
  const format = date.getFullYear() === new Date(now).getFullYear() ? DAY_MONTH : DAY_MONTH_YEAR;
  return `le ${format.format(date)}`;
}

/** La date et l'heure complètes de `at`, pour l'infobulle. */
export function fullDate(at: number): string {
  return FULL.format(new Date(at));
}
