// Les dates d'une mention (Task C4, R616). Une mention de date porte le jour dans son `id`,
// `AAAA-MM-JJ` (MMD : `<mention type="date" id="2026-12-01">1 déc. 2026</mention>`), et se montre
// par `dateLabel`. Un jour est une date du calendrier, sans heure ni fuseau : ici, un `Date` à minuit,
// heure locale.
//
// `parseDate` lit ce qu'on dit en entier : « aujourd'hui », « demain », « hier », un jour de la
// semaine (le prochain, jamais aujourd'hui), `JJ/MM` (cette année), `JJ/MM/AAAA`, `AAAA-MM-JJ` ; sans
// accents ni casse ; une date impossible (`31/02`) n'en est pas une. `suggestDates` est ce que le menu
// `@` propose pendant qu'on tape : la même lecture, plus les mots et les dates en cours de frappe.

/** Les jours de la semaine, dans l'ordre de `Date.getDay()`. */
const WEEKDAYS = ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];

const MONTHS = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

/** Sans accents, sans casse, sans apostrophe ni espaces de bord. */
function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/['’]/g, "")
    .trim()
    .toLowerCase();
}

/** Le jour `day` du mois `month` (de 1 à 12) de l'année `year`, s'il existe. */
function dayOf(year: number, month: number, day: number): Date | null {
  if (year < 1 || year > 9999) return null;
  const date = new Date(2000, 0, 1);
  // `new Date(26, …)` lirait 1926.
  date.setFullYear(year, month - 1, day);
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day ? date : null;
}

/** `today`, `days` jours plus tard (ou plus tôt), à minuit. */
function shifted(today: Date, days: number): Date {
  const date = new Date(2000, 0, 1);
  date.setFullYear(today.getFullYear(), today.getMonth(), today.getDate() + days);
  return date;
}

/** Les mots qui disent un jour, dans l'ordre où le menu les propose, et de combien ils décalent. */
function words(today: Date): { word: string; said: string; days: number }[] {
  const next = (weekday: number) => (weekday - today.getDay() + 7) % 7 || 7;
  return [
    { word: "aujourdhui", said: "Aujourd'hui", days: 0 },
    { word: "demain", said: "Demain", days: 1 },
    { word: "hier", said: "Hier", days: -1 },
    // De lundi à dimanche.
    ...[1, 2, 3, 4, 5, 6, 0].map((weekday) => ({
      word: WEEKDAYS[weekday],
      said: WEEKDAYS[weekday][0].toUpperCase() + WEEKDAYS[weekday].slice(1),
      days: next(weekday),
    })),
  ];
}

/** La date que dit `query`, lue un jour `today` ; `null` si elle n'en dit pas, ou une impossible. */
export function parseDate(query: string, today: Date): Date | null {
  const said = fold(query);
  const word = words(today).find((w) => w.word === said);
  if (word !== undefined) return shifted(today, word.days);
  let match = /^(\d{1,2})\/(\d{1,2})(?:\/(\d{4}))?$/.exec(said);
  if (match !== null) return dayOf(match[3] === undefined ? today.getFullYear() : Number(match[3]), Number(match[2]), Number(match[1]));
  match = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(said);
  if (match !== null) return dayOf(Number(match[1]), Number(match[2]), Number(match[3]));
  return null;
}

/** Une date comme elle se lit dans le texte : « 1 déc. 2026 ». */
export function dateLabel(date: Date): string {
  return `${date.getDate()} ${MONTHS[date.getMonth()]} ${date.getFullYear()}`;
}

/** L'`id` d'une mention de date : `AAAA-MM-JJ`. */
export function dateId(date: Date): string {
  const digits = (value: number, length: number) => String(value).padStart(length, "0");
  return `${digits(date.getFullYear(), 4)}-${digits(date.getMonth() + 1, 2)}-${digits(date.getDate(), 2)}`;
}

/** Le jour que nomme l'`id` d'une mention de date ; `null` s'il n'a pas exactement cette forme. */
export function dateOfId(id: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(id);
  return match === null ? null : dayOf(Number(match[1]), Number(match[2]), Number(match[3]));
}

/** Le jour `day` de ce mois (celui de `today`) s'il y existe, sinon du prochain mois qui l'a. */
function nextMonthWith(today: Date, day: number): Date | null {
  // Un jour de 1 à 31 est dans l'un des deux mois qui suivent (le 31 après un mois de trente jours,
  // le 29 ou le 30 après février).
  for (let ahead = 0; ahead <= 2; ahead++) {
    const month = new Date(2000, 0, 1);
    month.setFullYear(today.getFullYear(), today.getMonth() + ahead, 1);
    const date = dayOf(month.getFullYear(), month.getMonth() + 1, day);
    if (date !== null) return date;
  }
  return null;
}

/** Le jour `day` du mois `month` cette année (celle de `today`) s'il y existe, sinon la prochaine année qui l'a. */
function nextYearWith(today: Date, month: number, day: number): Date | null {
  // Le 29 février revient au plus tard en huit ans (1896, puis 1904).
  for (let ahead = 0; ahead <= 8; ahead++) {
    const date = dayOf(today.getFullYear() + ahead, month, day);
    if (date !== null) return date;
  }
  return null;
}

/** Une date que le menu propose, et le mot qui la dit (« Demain », « Lundi ») s'il y en a un. */
export type DateSuggestion = { date: Date; said: string | null };

/**
 * Les dates que le menu `@` propose pour `query`, un jour `today`. Rien de tapé : aujourd'hui et
 * demain. Un mot en cours de frappe : les mots qui commencent ainsi. Des chiffres : la date qu'ils
 * disent déjà (`12` : le 12 de ce mois ; `01/12/20` : le 1ᵉʳ décembre de cette année, en attendant
 * l'année ; `2026` et `2026-0` : le premier de l'année ; `2026-12` : le premier du mois). Un jour que
 * ce mois n'a pas est celui du prochain mois qui l'a (`31` en novembre : le 31 décembre), un jour
 * que cette année n'a pas celui de la prochaine année qui l'a (`29/02` : le prochain 29 février,
 * R768). Le menu de BlockNote se ferme quand la requête a plus de trois caractères de plus que la
 * dernière qui proposait quelque chose : une date qu'on tape en chiffres, sous l'une des formes que
 * lit `parseDate`, en garde donc une tout du long.
 */
export function suggestDates(query: string, today: Date): DateSuggestion[] {
  const said = fold(query);
  if (said === "") return words(today).slice(0, 2).map((w) => ({ date: shifted(today, w.days), said: w.said }));
  const completed = words(today).filter((w) => w.word.startsWith(said));
  if (completed.length > 0) return completed.map((w) => ({ date: shifted(today, w.days), said: w.said }));
  const one = (date: Date | null) => (date === null ? [] : [{ date, said: null }]);
  const exact = parseDate(said, today);
  if (exact !== null) return one(exact);
  let match = /^(\d{1,2})\/?$/.exec(said);
  if (match !== null) return one(nextMonthWith(today, Number(match[1])));
  match = /^(\d{1,2})\/(\d{1,2})(?:\/\d{0,3})?$/.exec(said);
  if (match !== null) return one(nextYearWith(today, Number(match[2]), Number(match[1])));
  match = /^(\d{4})(?:-0?)?$/.exec(said);
  if (match !== null) return one(dayOf(Number(match[1]), 1, 1));
  match = /^(\d{4})-(\d{1,2})-?$/.exec(said);
  if (match !== null) return one(dayOf(Number(match[1]), Number(match[2]), 1));
  return [];
}
