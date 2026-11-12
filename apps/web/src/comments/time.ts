// L'âge d'un message de commentaire, ou d'une version de l'historique, « il y a… », écrit à la
// main en français : jusqu'à une semaine, en minutes, heures ou jours ; au-delà, sa date. La date
// complète va dans l'infobulle.
import { useEffect, useState } from "react";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** « il y a… » se remet à jour à ce rythme, tant que ce qui l'affiche est ouvert. */
const CLOCK_MS = 30_000;

/** L'instant présent (millisecondes Unix), remis à jour toutes les `CLOCK_MS`. */
export function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => clearInterval(timer);
  }, []);
  return now;
}

const DAY_MONTH = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "long" });
const DAY_MONTH_YEAR = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "long",
  year: "numeric",
});
const FULL = new Intl.DateTimeFormat("fr-FR", { dateStyle: "long", timeStyle: "short" });

/** Le texte d'une date que `Date` ne sait pas dire (pas un nombre, ou hors de ±8,64e15 ms). */
const UNKNOWN = "date inconnue";

function valid(at: number): boolean {
  return Number.isFinite(at) && !Number.isNaN(new Date(at).getTime());
}

function ago(count: number, one: string, many: string): string {
  return `il y a ${count} ${count === 1 ? one : many}`;
}

/** L'âge de `at` à l'instant `now` (millisecondes Unix). Une date à venir vaut « à l'instant ». */
export function relativeTime(at: number, now: number): string {
  if (!valid(at) || !valid(now)) return UNKNOWN;
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
  return valid(at) ? FULL.format(new Date(at)) : UNKNOWN;
}

/** `at` au format ISO, pour l'attribut `dateTime` ; `undefined` pour une date que `Date` ne sait pas dire. */
export function isoDate(at: number): string | undefined {
  return valid(at) ? new Date(at).toISOString() : undefined;
}
