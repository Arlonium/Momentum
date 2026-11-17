// Dire une date en mots ou en chiffres (Task C4) : ce que lit `parseDate`, ce qu'écrit `dateLabel`,
// l'identifiant d'une mention de date, et ce que le menu `@` propose pendant qu'on tape.
import { expect, test } from "vitest";
import { dateId, dateLabel, dateOfId, parseDate, suggestDates } from "./dates";

/** Une date du calendrier local. */
const day = (y: number, m: number, d: number) => new Date(y, m - 1, d);

/** Le lundi 16 novembre 2026, en milieu de journée. */
const MONDAY = new Date(2026, 10, 16, 15, 30);

const read = (query: string, today = MONDAY) => {
  const date = parseDate(query, today);
  return date === null ? null : dateId(date);
};

test("parseDate reads the words, without accents or case", () => {
  expect(read("aujourd'hui")).toBe("2026-11-16");
  expect(read("Aujourd’hui")).toBe("2026-11-16");
  expect(read("AUJOURDHUI")).toBe("2026-11-16");
  expect(read("  demain ")).toBe("2026-11-17");
  expect(read("Hier")).toBe("2026-11-15");
});

test("parseDate reads a weekday as the next one, never today", () => {
  // « lundi » tapé un lundi : le suivant.
  expect(read("lundi")).toBe("2026-11-23");
  expect(read("mardi")).toBe("2026-11-17");
  expect(read("Mercredi")).toBe("2026-11-18");
  expect(read("jeudi")).toBe("2026-11-19");
  expect(read("vendredi")).toBe("2026-11-20");
  expect(read("samedi")).toBe("2026-11-21");
  expect(read("DIMANCHE")).toBe("2026-11-22");
  // Un dimanche : « lundi » est le lendemain, « dimanche » dans sept jours.
  const sunday = day(2026, 11, 22);
  expect(read("lundi", sunday)).toBe("2026-11-23");
  expect(read("dimanche", sunday)).toBe("2026-11-29");
});

test("parseDate reads the three written forms", () => {
  expect(read("01/12")).toBe("2026-12-01");
  expect(read("1/2")).toBe("2026-02-01");
  expect(read("01/12/2027")).toBe("2027-12-01");
  expect(read("2026-12-01")).toBe("2026-12-01");
  expect(read("2026-2-9")).toBe("2026-02-09");
});

test("parseDate crosses the end of a year and knows leap years", () => {
  const lastDay = new Date(2026, 11, 31, 23, 59);
  expect(read("demain", lastDay)).toBe("2027-01-01");
  expect(read("31/12", lastDay)).toBe("2026-12-31");
  expect(read("hier", day(2027, 1, 1))).toBe("2026-12-31");
  // Le jeudi 31 décembre 2026 : vendredi est en 2027.
  expect(read("vendredi", lastDay)).toBe("2027-01-01");
  expect(read("29/02/2028")).toBe("2028-02-29");
  expect(read("29/02/2027")).toBeNull();
  expect(read("29/02", day(2028, 1, 10))).toBe("2028-02-29");
  expect(read("demain", day(2028, 2, 28))).toBe("2028-02-29");
  expect(read("2100-02-29")).toBeNull();
});

test("parseDate reads nothing else", () => {
  for (const query of [
    "",
    " ",
    "31/02",
    "32/01",
    "00/01",
    "01/13",
    "01/00",
    "1/2/26",
    "01/12/20267",
    "2026-13-01",
    "2026-00-10",
    "2026-12-32",
    "26-12-01",
    "0000-01-01",
    "lundis",
    "lun",
    "après-demain",
    "tomorrow",
    "1",
    "01/12/",
    "2026-12",
    "01-12-2026",
    "1e3",
    "١٢/٠١",
    "<b>demain</b>",
  ]) {
    expect(read(query), query).toBeNull();
  }
});

test("a date is written 1 déc. 2026, and its id is AAAA-MM-JJ", () => {
  expect(dateLabel(day(2026, 12, 1))).toBe("1 déc. 2026");
  expect(dateLabel(day(2027, 1, 31))).toBe("31 janv. 2027");
  expect(dateLabel(day(2026, 5, 9))).toBe("9 mai 2026");
  expect(dateLabel(day(2026, 8, 15))).toBe("15 août 2026");
  expect(dateId(day(2026, 12, 1))).toBe("2026-12-01");
  expect(dateId(day(987, 3, 4))).toBe("0987-03-04");
});

test("the id of a date mention is read back strictly", () => {
  expect(dateId(dateOfId("2026-12-01")!)).toBe("2026-12-01");
  expect(dateId(dateOfId("2028-02-29")!)).toBe("2028-02-29");
  for (const id of ["", "2026-12-1", "2026-02-30", "01/12/2026", "demain", "2026-12-01T00:00", " 2026-12-01", "20261201"]) {
    expect(dateOfId(id), id).toBeNull();
  }
});

/** Ce que le menu propose : l'id de chaque date, et ce qu'elle dit. */
const offered = (query: string, today = MONDAY) =>
  suggestDates(query, today).map((s) => (s.said === null ? dateId(s.date) : `${dateId(s.date)} ${s.said}`));

test("the menu offers today and tomorrow before anything is typed", () => {
  expect(offered("")).toEqual(["2026-11-16 Aujourd'hui", "2026-11-17 Demain"]);
});

test("the menu completes a word being typed", () => {
  expect(offered("d")).toEqual(["2026-11-17 Demain", "2026-11-22 Dimanche"]);
  expect(offered("dem")).toEqual(["2026-11-17 Demain"]);
  expect(offered("Aujourd")).toEqual(["2026-11-16 Aujourd'hui"]);
  expect(offered("mer")).toEqual(["2026-11-18 Mercredi"]);
  expect(offered("lundi")).toEqual(["2026-11-23 Lundi"]);
  expect(offered("x")).toEqual([]);
  expect(offered("demains")).toEqual([]);
});

test("the menu keeps a date on offer while its digits are typed", () => {
  // JJ/MM/AAAA, frappe après frappe : jamais quatre frappes de suite sans date (BlockNote ferme
  // alors le menu).
  expect(["0", "01", "01/", "01/1", "01/12", "01/12/", "01/12/2", "01/12/20", "01/12/202", "01/12/2027"].map((q) => offered(q))).toEqual([
    [],
    ["2026-11-01"],
    ["2026-11-01"],
    ["2026-01-01"],
    ["2026-12-01"],
    ["2026-12-01"],
    ["2026-12-01"],
    ["2026-12-01"],
    ["2026-12-01"],
    ["2027-12-01"],
  ]);
  // AAAA-MM-JJ : l'année seule vaut son premier jour, le mois seul aussi (R768).
  expect(["2", "20", "202", "2026", "2026-", "2026-1", "2026-12", "2026-12-", "2026-12-0", "2026-12-01"].map((q) => offered(q))).toEqual([
    ["2026-11-02"],
    ["2026-11-20"],
    [],
    ["2026-01-01"],
    ["2026-01-01"],
    ["2026-01-01"],
    ["2026-12-01"],
    ["2026-12-01"],
    [],
    ["2026-12-01"],
  ]);
  // Une date impossible n'est jamais proposée.
  expect(offered("31/02")).toEqual([]);
  expect(offered("31/02/2026")).toEqual([]);
  expect(offered("32")).toEqual([]);
  expect(offered("00")).toEqual([]);
});

test("a day this month does not have is offered in the next month that has it", () => {
  // `@31` le 15 de chaque mois de 2026 : le 31 de ce mois, ou du prochain qui en a un (R768). Il ne
  // proposait rien un mois de trente jours, ni le 29 ou le 30 en février.
  const thirtyFirst = Array.from({ length: 12 }, (_, m) => offered("31", day(2026, m + 1, 15))[0]);
  expect(thirtyFirst).toEqual([
    "2026-01-31",
    "2026-03-31",
    "2026-03-31",
    "2026-05-31",
    "2026-05-31",
    "2026-07-31",
    "2026-07-31",
    "2026-08-31",
    "2026-10-31",
    "2026-10-31",
    "2026-12-31",
    "2026-12-31",
  ]);
  expect(offered("31/", day(2026, 11, 30))).toEqual(["2026-12-31"]);
  expect(offered("30", day(2026, 2, 3))).toEqual(["2026-03-30"]);
  expect(offered("29", day(2026, 2, 3))).toEqual(["2026-03-29"]);
  expect(offered("29", day(2028, 2, 3))).toEqual(["2028-02-29"]);
  // Le 31 décembre, `@31` est le jour même ; en décembre, le prochain 31 est en janvier.
  expect(offered("31", day(2026, 12, 31))).toEqual(["2026-12-31"]);
  // Le 29 février d'une année qui ne l'a pas : la prochaine année qui l'a, tant que l'année se tape.
  expect(["29/02", "29/02/", "29/02/2", "29/02/20", "29/02/202", "29/02/2028"].map((q) => offered(q))).toEqual(
    Array.from({ length: 6 }, () => ["2028-02-29"]),
  );
  // L'année seule, ou suivie du début de son mois, vaut son premier jour.
  expect(["2026", "2026-", "2026-0", "2026-01", "2026-01-1", "2026-01-15"].map((q) => offered(q))).toEqual([
    ["2026-01-01"],
    ["2026-01-01"],
    ["2026-01-01"],
    ["2026-01-01"],
    ["2026-01-01"],
    ["2026-01-15"],
  ]);
});

/**
 * La règle de BlockNote (`useCloseSuggestionMenuNoItems`) : le menu se ferme quand la requête a plus
 * de trois caractères de plus que la dernière qui proposait quelque chose. Rend la première requête
 * qui le fermerait, ou `null`.
 */
function closesAt(typed: string, today: Date): string | null {
  let useful = 0;
  for (let length = 1; length <= typed.length; length++) {
    const query = typed.slice(0, length);
    if (suggestDates(query, today).length > 0) useful = length;
    else if (length - useful > 3) return query;
  }
  return null;
}

test("a date typed in digits keeps the menu open on every day of every month", () => {
  // Chaque jour de 2027 et de 2028 (bissextile), tapé de six façons, frappe après frappe, un jour de
  // chaque mois de 2027 (le premier et le dernier) : le menu ne se ferme jamais avant la fin, et la
  // date entière, avec son année, est celle qu'on a tapée.
  const pad = (n: number) => String(n).padStart(2, "0");
  const todays = Array.from({ length: 12 }, (_, m) => [day(2027, m + 1, 1), new Date(2027, m + 1, 0)]).flat();
  const failures: string[] = [];
  for (let at = day(2027, 1, 1); at <= day(2028, 12, 31); at = new Date(at.getFullYear(), at.getMonth(), at.getDate() + 1)) {
    const [y, m, d] = [at.getFullYear(), at.getMonth() + 1, at.getDate()];
    const withYear = [`${pad(d)}/${pad(m)}/${y}`, `${d}/${m}/${y}`, `${y}-${pad(m)}-${pad(d)}`, `${y}-${m}-${d}`];
    const withoutYear = [`${pad(d)}/${pad(m)}`, `${d}/${m}`];
    for (const today of todays) {
      for (const typed of [...withYear, ...withoutYear]) {
        const closed = closesAt(typed, today);
        if (closed !== null) failures.push(`${typed} le ${dateId(today)} : fermé à « ${closed} »`);
      }
      for (const typed of withYear) {
        const last = suggestDates(typed, today);
        if (last.length !== 1 || dateId(last[0].date) !== dateId(at)) failures.push(`${typed} le ${dateId(today)} : ${offered(typed, today)}`);
      }
    }
  }
  expect(failures.slice(0, 10)).toEqual([]);
});
