// « il y a… » : l'âge d'un message, en français, écrit à la main (aucune dépendance).
import { expect, test } from "vitest";
import { relativeTime } from "./time";

const SECOND = 1_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Le 12 novembre 2026 à midi, heure locale. */
const NOW = new Date(2026, 10, 12, 12, 0, 0).getTime();

test("recent_messages_say_how_long_ago", () => {
  expect(relativeTime(NOW, NOW)).toBe("à l'instant");
  expect(relativeTime(NOW - 59 * SECOND, NOW)).toBe("à l'instant");
  expect(relativeTime(NOW - MINUTE, NOW)).toBe("il y a 1 minute");
  expect(relativeTime(NOW - 59 * MINUTE, NOW)).toBe("il y a 59 minutes");
  expect(relativeTime(NOW - HOUR, NOW)).toBe("il y a 1 heure");
  expect(relativeTime(NOW - 23 * HOUR, NOW)).toBe("il y a 23 heures");
  expect(relativeTime(NOW - DAY, NOW)).toBe("il y a 1 jour");
  expect(relativeTime(NOW - 6 * DAY, NOW)).toBe("il y a 6 jours");
});

test("older_messages_give_their_date", () => {
  expect(relativeTime(new Date(2026, 10, 3, 9, 0).getTime(), NOW)).toBe("le 3 novembre");
  expect(relativeTime(new Date(2025, 0, 1, 9, 0).getTime(), NOW)).toBe("le 1 janvier 2025");
});

test("a_date_from_the_future_is_now", () => {
  // Horloge du poste en retard sur celle du serveur.
  expect(relativeTime(NOW + 5 * MINUTE, NOW)).toBe("à l'instant");
});
