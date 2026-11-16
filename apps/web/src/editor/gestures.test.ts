// Ce que les gestes de la partie B du lot 3 écrivent dans le document partagé, et ce que devient
// une frappe faite au même instant par quelqu'un d'autre (plan de la partie B, « Global
// Constraints », « Gestes de l'éditeur »). Toute tâche de la partie B qui ajoute un geste commence
// ici, par le test qui le relit dans le document : insérer des blocs (C1a ; dupliquer, C5, à la fin), changer
// des props (C1a ; la formule d'une équation, C2 ; l'adresse d'un fichier, C8), changer le type
// d'un bloc (C1a ; une image devenue fichier, un fichier déposé sur un paragraphe vide, C8), poser
// un contenu en ligne (une formule, C2 ; une mention, C4). Chacune ajoute sa section, à la suite.
//
// Mesuré le 2026-11-13 avec y-prosemirror 1.3.7 **corrigé par la Task 13a**
// (`patches/y-prosemirror@1.3.7.patch`, R220) et BlockNote 0.57. Le correctif ne joue que si un
// bloc présent avant et après une transaction a changé d'ordre (R223) ; aucun geste de ce fichier
// n'en déplace un : c'est le code d'origine de y-prosemirror qui écrit, et ces mesures valent sans
// le correctif. Elles redisent celles que le plan avait prises sans lui (« Ce qui a été vérifié
// avant d'écrire »), à une précision près : de deux changements de la même prop faits en même
// temps, ce n'est pas « le dernier » qui reste, mais celui du plus grand identifiant de client.
//
// Banc (`test/editors.ts`) : deux vrais éditeurs BlockNote, ceux de Mona et de Tom, liés à deux
// `Y.Doc` aux identifiants de client fixes, et le document du « serveur », lu sans éditeur ; leurs
// mises à jour s'échangent à la main. Chacun fait son geste depuis le même état, sans avoir vu
// celui de l'autre. Chaque mesure est jouée dans les deux ordres d'identifiants de client (l'issue
// d'un conflit en dépend) et dans les deux ordres d'arrivée au serveur (`PLAYS`). Ce qui est relu :
// la page chez Mona et chez Tom (leurs éditeurs) et au serveur (son document, sans éditeur), les
// props dans les trois documents et dans les deux éditeurs, les trois documents égaux, et les
// éléments Yjs de chaque bloc (`elementIds`) : « aucun élément existant n'est touché » se lit là,
// pas dans les ids, qu'un élément peut passer d'un bloc à un autre sans que rien ne bouge (R223).
//
// Une limite (le comportement voulu n'est pas tenu) est un `test.fails` dont seul l'échec attendu,
// mot pour mot, compte pour l'échec (`knownLimit`) : une précondition manquée, une erreur d'un
// éditeur, des documents qui divergent ou un autre manque le font passer au rouge, comme le jour
// où la limite est levée.
import { describe, expect, test } from "vitest";
import * as Y from "yjs";
import {
  block,
  counts,
  cursor,
  drag,
  duo,
  elementIds,
  exchange,
  line,
  PARENTED,
  repeatedIds,
  shown,
  shownInline,
  stored,
  storedInline,
  storedProps,
  typeAt,
  typeAtEnd,
  written,
  X,
  xml,
  XYZ,
  Y_,
  Z,
  type Peer,
  type Seed,
  type Shown,
} from "../test/editors";
import { FRAGMENT } from "./collab";
import { duplicateBlocks } from "./duplicate";

/** Ce que Tom tape. */
const TYPED = " de Tom";

/** Les deux ordres des identifiants de client de Mona et de Tom (la page de départ : 1). */
const ORDERS = [
  { label: "Mona avant Tom", mona: 2, tom: 3 },
  { label: "Tom avant Mona", mona: 3, tom: 2 },
] as const;
type Order = (typeof ORDERS)[number];

/** Qui arrive le premier au serveur, et chez l'autre. */
type First = "mona" | "tom";

/** Chaque ordre d'identifiants, avec chaque ordre d'arrivée. */
const PLAYS = ORDERS.flatMap((order) =>
  (["mona", "tom"] as const).map((first: First) => ({
    order,
    first,
    label: `${order.label}, ${first === "mona" ? "Mona" : "Tom"} arrive le premier`,
  })),
);

// ---------------------------------------------------------------------------
// Les pages de départ
// ---------------------------------------------------------------------------

// « Bloc X » (avec son enfant X1), « Bloc Y », « Bloc Z » : `PARENTED`, `XYZ` (`test/editors.ts`).

/** Un callout entre deux paragraphes, avec ses props par défaut (celles du registre). */
const CALLOUT: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "c", type: "callout", content: "Attention" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
const DEFAULT_CALLOUT = { icon: "💡", color: "default" };

/** Un paragraphe vide entre deux autres : celui où l'on tape « / » (C1b). */
const EMPTY: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "e", type: "paragraph", content: "" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];

const callout = (id: string, text: string) => block(id, text, { type: "callout" });

// ---------------------------------------------------------------------------
// Une scène, ses préconditions, son exigence
// ---------------------------------------------------------------------------

/** La page `blocks` ouverte par Mona et Tom, aux identifiants de client de `order`. */
function scene(blocks: Seed, order: Order) {
  return duo(blocks, order.mona, order.tom);
}

type Scene = Awaited<ReturnType<typeof scene>>;

/** Les trois documents, nommés pour les messages. */
function docs(s: Scene): [string, Y.Doc][] {
  return [
    ["dans le document de Mona", s.mona.doc],
    ["dans le document de Tom", s.tom.doc],
    ["au serveur", s.server],
  ];
}

/** Ce que chacun a écrit part au serveur et chez l'autre ; `first` arrive le premier. */
function send(s: Scene, first: First): Promise<void> {
  return exchange(s.server, first === "mona" ? [s.mona, s.tom] : [s.tom, s.mona]);
}

/** Ce que `p` a écrit et pas encore envoyé, en une mise à jour, sans le retirer de l'envoi. */
function pending(p: Peer): Uint8Array {
  return Y.mergeUpdates(p.writes);
}

/** Une précondition manquée : le test prouverait autre chose que ce qu'il dit. */
class Missed extends Error {}

/** L'exigence d'une mesure n'est pas tenue : une ligne par manque. */
class Unmet extends Error {}

/**
 * Personne n'a rien écrit en ouvrant la page, et chacun montre, après son geste et avant
 * l'échange, la page `local` dit. Sinon, lève `Missed`, qui n'est jamais l'échec attendu.
 */
function requireLocallyDone(s: Scene, local: { mona: Shown[]; tom: Shown[] }): void {
  const same = (p: Peer, page: Shown[]) => JSON.stringify(shown(p)) === JSON.stringify(page);
  if (s.quiet && same(s.mona, local.mona) && same(s.tom, local.tom)) return;
  throw new Missed(
    `précondition manquée : ouverture sans écriture ${s.quiet} ; ` +
      `Mona ${line(shown(s.mona))} (attendu : ${line(local.mona)}) ; ` +
      `Tom ${line(shown(s.tom))} (attendu : ${line(local.tom)})`,
  );
}

/** Une ligne de manque de la page : ce que `where` montre, et ce qui est voulu. */
const pageLine = (where: string, seen: Shown[], wanted: Shown[]) =>
  `${where} : la page voulue (vu : ${line(seen)} ; voulu : ${line(wanted)})`;

/**
 * Ce qui manque, après l'échange, une ligne par manque : la page `wanted` chez Mona et chez Tom
 * (leurs éditeurs) et au serveur (son document, sans éditeur), types et enfants compris, chaque id
 * une seule fois ; les props `props` (par id de bloc) dans les trois documents et dans les deux
 * éditeurs ; les trois documents identiques.
 */
function unmet(s: Scene, wanted: Shown[], props: Record<string, Record<string, unknown>> = {}): string[] {
  const lines: string[] = [];
  const views: [string, Shown[]][] = [
    ["chez Mona", shown(s.mona)],
    ["chez Tom", shown(s.tom)],
    ["au serveur", stored(s.server)],
  ];
  for (const [where, page] of views) {
    const repeated = repeatedIds(page);
    if (repeated.length > 0) lines.push(`${where} : chaque id une seule fois (en double : ${repeated.join(", ")})`);
    if (JSON.stringify(page) !== JSON.stringify(wanted)) lines.push(pageLine(where, page, wanted));
  }
  for (const [id, want] of Object.entries(props)) {
    const seen: [string, unknown][] = [
      ...docs(s).map(([where, doc]): [string, unknown] => [where, storedProps(doc, id)]),
      ["dans l'éditeur de Mona", s.mona.editor.getBlock(id)?.props],
      ["dans l'éditeur de Tom", s.tom.editor.getBlock(id)?.props],
    ];
    for (const [where, value] of seen) {
      if (JSON.stringify(value) !== JSON.stringify(want)) {
        lines.push(`${where} : les props de ${id} (vu : ${JSON.stringify(value)} ; voulu : ${JSON.stringify(want)})`);
      }
    }
  }
  if (xml(s.tom.doc) !== xml(s.mona.doc)) lines.push("Mona et Tom n'ont pas le même document");
  if (xml(s.server) !== xml(s.mona.doc)) lines.push("le serveur n'a pas le document de Mona");
  return lines;
}

/** La page voulue partout (`unmet`) ; sinon, lève `Unmet`, avec tout ce qui manque. */
function expectEverywhere(
  s: Scene,
  wanted: Shown[],
  props: Record<string, Record<string, unknown>> = {},
): void {
  const lines = unmet(s, wanted, props);
  if (lines.length > 0) throw new Unmet(lines.join("\n"));
}

/**
 * Les blocs de `before` (leurs éléments Yjs, `elementIds`) qui n'ont plus, dans `doc`, les mêmes
 * éléments : une ligne par bloc, vide si aucun n'a été touché.
 */
function changedElements(doc: Y.Doc, before: Map<string, string>): string[] {
  const after = elementIds(doc);
  return [...before]
    .filter(([id, was]) => after.get(id) !== was)
    .map(([id, was]) => `${id} : ${was} → ${after.get(id) ?? "absent"}`);
}

/** Les ids de bloc de `doc` qui ne sont pas dans `before`, dans l'ordre alphabétique. */
function newIds(doc: Y.Doc, before: Map<string, string>): string[] {
  return [...elementIds(doc).keys()].filter((id) => !before.has(id)).sort();
}

/**
 * Ce que donne une mesure : `juste` (le comportement voulu est tenu), `perte` (il ne l'est pas,
 * exactement comme la limite le dit : `Unmet`, avec les lignes attendues, mot pour mot), ou
 * `accident` (toute autre exception, `Unmet` d'un autre manque compris).
 */
type Outcome =
  | { kind: "juste" }
  | { kind: "perte"; error: Unmet }
  | { kind: "accident"; error: unknown };

async function outcomeOf(body: () => Promise<void>, loss: string): Promise<Outcome> {
  try {
    await body();
    return { kind: "juste" };
  } catch (error) {
    return error instanceof Unmet && error.message === loss
      ? { kind: "perte", error }
      : { kind: "accident", error };
  }
}

/**
 * Une limite : le comportement voulu (la page `wanted`) n'est pas tenu, et l'on voit partout, à la
 * place, la page `lost`, pour la raison `reason`. `test.fails` tient toute exception pour l'échec
 * attendu : seule la perte dite lui est donc rendue, et elle seule (les trois lignes « la page
 * voulue », chez Mona, chez Tom et au serveur, avec `lost` vu ; les documents identiques). Un
 * accident est écrit au journal, et le test rend la main sans échouer, ce que `test.fails` signale
 * en rouge, comme une limite levée (à repasser en `test`, et à consigner).
 */
function knownLimit(
  name: string,
  reason: string,
  { wanted, lost }: { wanted: Shown[]; lost: Shown[] },
  body: () => Promise<void>,
): void {
  const loss = ["chez Mona", "chez Tom", "au serveur"].map((w) => pageLine(w, lost, wanted)).join("\n");
  test.fails(`${name} [limite : ${reason}]`, async () => {
    const outcome = await outcomeOf(body, loss);
    if (outcome.kind === "perte") throw outcome.error;
    if (outcome.kind === "accident") {
      console.error(`${name} : pas la perte attendue, un accident :`, outcome.error);
    }
  });
}

// ---------------------------------------------------------------------------
// La cible elle-même
// ---------------------------------------------------------------------------

// Ce sur quoi reposent les mesures : la comparaison des éléments voit un élément recréé, et
// seulement lui ; l'exigence voit la page, les props et les documents ; seule la perte dite d'une
// limite passe pour son échec.
describe("la cible : ce qu'une mesure voit", () => {
  const order = ORDERS[0];

  test("elementIds : une frappe ne change aucun élément", async () => {
    const s = await scene(XYZ, order);
    const before = elementIds(s.server);
    expect([...before.keys()]).toEqual(["x", "y", "z"]);
    typeAtEnd(s.mona, "y", TYPED);
    expect(changedElements(s.mona.doc, before)).toEqual([]);
  });

  test("elementIds : un changement de type garde le conteneur et change l'élément de contenu", async () => {
    const s = await scene(XYZ, order);
    const before = elementIds(s.server);
    s.mona.editor.updateBlock("y", { type: "callout" });
    const [was, now] = [before.get("y")!.split(" "), elementIds(s.mona.doc).get("y")!.split(" ")];
    expect(now[0], "le même conteneur").toBe(was[0]);
    expect([was[1], now[1]]).toEqual(["paragraph", "callout"]);
    expect(now[2], "un autre élément de contenu").not.toBe(was[2]);
    expect(changedElements(s.mona.doc, before)).toEqual([`y : ${before.get("y")} → ${elementIds(s.mona.doc).get("y")}`]);
  });

  test("elementIds : un bloc glissé change de conteneur, les autres non", async () => {
    const s = await scene(XYZ, order);
    const before = elementIds(s.server);
    drag(s.mona, "x", { under: "z" });
    expect(changedElements(s.mona.doc, before).map((l) => l.split(" ")[0])).toEqual(["x"]);
  });

  test("elementIds : un id en double se voit", async () => {
    const s = await scene(XYZ, order);
    // Écrit au document du serveur, sans éditeur, qui donnerait un id neuf au second.
    const group = s.server.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement;
    const twin = new Y.XmlElement("blockContainer");
    twin.setAttribute("id", "y");
    twin.insert(0, [new Y.XmlElement("paragraph")]);
    group.insert(3, [twin]);
    expect(elementIds(s.server).get("y")?.split(" ; ")).toHaveLength(2);
  });

  test("l'exigence voit la page, les props et les documents qui diffèrent", async () => {
    const s = await scene(CALLOUT, order);
    expect(unmet(s, [X, callout("c", "Attention"), Z], { c: DEFAULT_CALLOUT })).toEqual([]);
    // Le même texte dans un paragraphe n'est pas la page, chez personne.
    const flat = unmet(s, [X, block("c", "Attention"), Z]);
    expect(flat.filter((l) => l.includes("la page voulue"))).toHaveLength(3);
    // D'autres props : vues dans les trois documents et les deux éditeurs.
    const other = unmet(s, [X, callout("c", "Attention"), Z], { c: { ...DEFAULT_CALLOUT, color: "blue" } });
    expect(other.filter((l) => l.includes("les props de c"))).toHaveLength(5);
    // Un document qui a reçu ce que l'autre n'a pas.
    typeAtEnd(s.mona, "x", TYPED);
    expect(unmet(s, [X, callout("c", "Attention"), Z])).toContain("Mona et Tom n'ont pas le même document");
  });

  test("seule la perte dite d'une limite est son échec ; une précondition manquée ou un autre manque sont des accidents", async () => {
    const wanted = [X, block("y", `Bloc Y${TYPED}`), Z];
    const lost = [X, Y_, Z];
    const loss = ["chez Mona", "chez Tom", "au serveur"].map((w) => pageLine(w, lost, wanted)).join("\n");
    // Tom tape dans Y, ou ne fait rien ; Tom montre `local` avant l'échange, `want` est exigé.
    const play = async (types: boolean, local: Shown[], want: Shown[]) => {
      const s = await scene(XYZ, order);
      if (types) typeAtEnd(s.tom, "y", TYPED);
      requireLocallyDone(s, { mona: [X, Y_, Z], tom: local });
      await send(s, "tom");
      expectEverywhere(s, want);
    };
    // La frappe arrive partout : juste.
    expect((await outcomeOf(() => play(true, wanted, wanted), loss)).kind).toBe("juste");
    // Sans frappe, la page sans elle est vue partout à la place de la page voulue : la perte dite.
    const said = await outcomeOf(() => play(false, lost, wanted), loss);
    expect(said.kind).toBe("perte");
    // Un autre manque (une autre page exigée) : un accident, même s'il lève `Unmet`.
    const other = await outcomeOf(() => play(true, wanted, [X, Z]), loss);
    expect(other.kind).toBe("accident");
    expect((other as { error: unknown }).error).toBeInstanceOf(Unmet);
    // Une précondition manquée (Tom devait ne rien montrer de neuf) : un accident.
    const missed = await outcomeOf(() => play(true, lost, lost), loss);
    expect(missed.kind).toBe("accident");
    expect((missed as { error: unknown }).error).toBeInstanceOf(Missed);
    // Une exception d'un geste : un accident.
    const broken = await outcomeOf(async () => {
      const s = await scene(XYZ, order);
      typeAtEnd(s.tom, "aucun", TYPED);
    }, loss);
    expect(broken.kind).toBe("accident");
    expect(String((broken as { error: unknown }).error)).toContain("aucun bloc aucun");
  });
});

// ---------------------------------------------------------------------------
// Insérer des blocs (C1a ; dupliquer un bloc, C5)
// ---------------------------------------------------------------------------

/**
 * Mona insère après X une copie de X sans aucun id, son enfant compris, puis un callout : ce que
 * font Ctrl+D (C5) et le menu « / » dans un bloc qui n'est pas vide (C1b, `insertBlocks` après le
 * bloc). Rend les ids que BlockNote leur a donnés : la copie, son enfant, le callout.
 */
function insertAfterX(p: Peer): [string, string, string] {
  const made = p.editor.insertBlocks(
    [
      { type: "paragraph", content: "Bloc X", children: [{ type: "paragraph", content: "Enfant X1" }] },
      { type: "callout", content: "Note" },
    ],
    "x",
    "after",
  );
  return [made[0].id, made[0].children[0].id, made[1].id];
}

/** X et son enfant, avec ce que Tom a tapé dans l'un et l'autre. */
const X_TYPED = block("x", `Bloc X${TYPED}`, { children: [block("x1", `Enfant X1${TYPED}`)] });
const X_AS_SEEDED = block("x", "Bloc X", { children: [block("x1", "Enfant X1")] });
const Y_TYPED = block("y", `Bloc Y${TYPED}`);

describe("insérer des blocs", () => {
  test.each(PLAYS)("inserting_blocks_creates_only_new_elements ($label)", async ({ order, first }) => {
    const s = await scene(PARENTED, order);
    const before = elementIds(s.server);
    let ids: [string, string, string] = ["", "", ""];
    const wrote = written(s.mona, () => {
      ids = insertAfterX(s.mona);
    });
    // Tom tape dans le bloc d'avant (X, et son enfant X1) et dans celui d'après (Y).
    typeAtEnd(s.tom, "x", TYPED);
    typeAtEnd(s.tom, "x1", TYPED);
    typeAtEnd(s.tom, "y", TYPED);
    const [copy, child, note] = ids;
    const inserted = [block(copy, "Bloc X", { children: [block(child, "Enfant X1")] }), callout(note, "Note")];
    requireLocallyDone(s, { mona: [X_AS_SEEDED, ...inserted, Y_, Z], tom: [X_TYPED, Y_TYPED, Z] });

    // Ce que le geste écrit : des éléments neufs, sous des ids neufs ; rien de supprimé, aucun id
    // réattribué ; rien d'autre que ces éléments, leurs attributs et leurs textes.
    expect(wrote).toEqual({
      deleted: [],
      created: [
        `blockContainer ${copy}`,
        `paragraph de ${copy}`,
        `blockGroup de ${copy}`,
        `blockContainer ${child}`,
        `paragraph de ${child}`,
        `blockContainer ${note}`,
        `callout de ${note}`,
      ],
      renamed: {},
    });
    expect(counts(pending(s.mona))).toMatchObject({
      made: {
        "<blockContainer>": 3,
        "<paragraph>": 2,
        "<blockGroup>": 1,
        "<callout>": 1,
        texte: 3,
        caractères: "Bloc X".length + "Enfant X1".length + "Note".length,
        "@id": 3,
        "@backgroundColor": 2,
        "@textColor": 2,
        "@textAlignment": 2,
        "@icon": 1,
        "@color": 1,
      },
      deleted: 0,
    });

    await send(s, first);
    // La frappe de Tom est gardée, dans X, dans X1 et dans Y ; les trois documents sont égaux.
    expectEverywhere(s, [X_TYPED, ...inserted, Y_TYPED, Z]);
    for (const [where, doc] of docs(s)) {
      // Aucun bloc qui existait ne change d'élément ; les blocs neufs, eux seuls, une fois chacun.
      expect(changedElements(doc, before), where).toEqual([]);
      expect(newIds(doc, before), where).toEqual([copy, child, note].sort());
      expect([...elementIds(doc).values()].filter((v) => v.includes(" ; ")), where).toEqual([]);
    }
  });

  test("témoin : la même comparaison verrait un bloc voisin récrit avec l'insertion", async () => {
    const s = await scene(PARENTED, ORDERS[0]);
    const before = elementIds(s.server);
    s.mona.editor.transact(() => {
      insertAfterX(s.mona);
      s.mona.editor.updateBlock("y", { type: "callout" });
    });
    await send(s, "mona");
    for (const [where, doc] of docs(s)) {
      expect(changedElements(doc, before).map((l) => l.split(" ")[0]), where).toEqual(["y"]);
    }
  });
});

// ---------------------------------------------------------------------------
// Changer des props (C1a ; la formule d'une équation, C2 ; l'adresse d'un fichier, C8)
// ---------------------------------------------------------------------------

describe("changer des props", () => {
  test.each(PLAYS)("changing_a_prop_keeps_the_element_and_what_another_types ($label)", async ({ order, first }) => {
    const s = await scene(CALLOUT, order);
    const before = elementIds(s.server);
    const wanted = { icon: "⚠️", color: "blue" };
    const wrote = written(s.mona, () => s.mona.editor.updateBlock("c", { props: wanted }));
    // Tom tape dans le callout pendant que Mona en change la couleur et l'icône.
    typeAtEnd(s.tom, "c", TYPED);
    requireLocallyDone(s, {
      mona: [X, callout("c", "Attention"), Z],
      tom: [X, callout("c", `Attention${TYPED}`), Z],
    });
    expect(s.mona.editor.getBlock("c")?.props, "chez Mona, avant l'échange").toEqual(wanted);

    // Ce que le geste écrit : les deux attributs de l'élément du callout, récrits, et rien d'autre ;
    // aucun élément créé ni supprimé.
    expect(wrote).toEqual({ deleted: [], created: [], renamed: {} });
    expect(counts(pending(s.mona))).toMatchObject({ made: { "attribut récrit": 2 }, deleted: 2 });

    await send(s, first);
    expectEverywhere(s, [X, callout("c", `Attention${TYPED}`), Z], { c: wanted });
    for (const [where, doc] of docs(s)) {
      expect(changedElements(doc, before), where).toEqual([]);
      expect(newIds(doc, before), where).toEqual([]);
    }
  });

  // Yjs range à droite, de deux écritures concurrentes de la même clé, celle du plus grand
  // identifiant de client, et la clé vaut la plus à droite (`Item.integrate`). Dans un navigateur,
  // où ces identifiants sont tirés au hasard, c'est l'une ou l'autre, la même partout.
  test.each(PLAYS)("two_changes_of_the_same_prop_keep_one_value_everywhere ($label)", async ({ order, first }) => {
    const s = await scene(CALLOUT, order);
    const before = elementIds(s.server);
    s.mona.editor.updateBlock("c", { props: { color: "blue" } });
    s.tom.editor.updateBlock("c", { props: { color: "red" } });
    requireLocallyDone(s, { mona: [X, callout("c", "Attention"), Z], tom: [X, callout("c", "Attention"), Z] });
    await send(s, first);
    const color = order.mona > order.tom ? "blue" : "red";
    expectEverywhere(s, [X, callout("c", "Attention"), Z], { c: { ...DEFAULT_CALLOUT, color } });
    for (const [where, doc] of docs(s)) expect(changedElements(doc, before), where).toEqual([]);
  });

  test("témoin : l'un après l'autre, le dernier changement gagne, quels que soient les identifiants", async () => {
    for (const order of ORDERS) {
      const s = await scene(CALLOUT, order);
      s.mona.editor.updateBlock("c", { props: { color: "blue" } });
      await send(s, "mona");
      s.tom.editor.updateBlock("c", { props: { color: "red" } });
      await send(s, "tom");
      expectEverywhere(s, [X, callout("c", "Attention"), Z], { c: { ...DEFAULT_CALLOUT, color: "red" } });
    }
  });

  test("témoin : la même comparaison verrait un changement de type à la place", async () => {
    const s = await scene(CALLOUT, ORDERS[0]);
    const before = elementIds(s.server);
    const wrote = written(s.mona, () => s.mona.editor.updateBlock("c", { type: "paragraph" }));
    expect(wrote).not.toEqual({ deleted: [], created: [], renamed: {} });
    await send(s, "mona");
    for (const [where, doc] of docs(s)) {
      expect(changedElements(doc, before).map((l) => l.split(" ")[0]), where).toEqual(["c"]);
    }
  });
});

// ---------------------------------------------------------------------------
// Changer le type d'un bloc (C1a ; une image devenue fichier, un fichier déposé sur un paragraphe
// vide, C8)
// ---------------------------------------------------------------------------

/**
 * Les changements de type mesurés : un paragraphe qui devient un callout, et un paragraphe vide qui
 * en devient un, comme le fait le menu « / » dans un bloc vide (BlockNote, `updateBlock` du bloc
 * courant ; dans un bloc qui n'est pas vide, il insère après, plus haut).
 */
const RETYPES = [
  {
    title: "un paragraphe devient un callout",
    seed: XYZ,
    id: "y",
    text: "Bloc Y",
    made: { "<callout>": 1, texte: 1, caractères: "Bloc Y".length, "@icon": 1, "@color": 1 },
    // L'élément du paragraphe, ses trois attributs, son texte et ses six caractères.
    deleted: 11,
  },
  {
    title: "un paragraphe vide devient un callout (menu « / »)",
    seed: EMPTY,
    id: "e",
    text: "",
    made: { "<callout>": 1, "@icon": 1, "@color": 1 },
    deleted: 4,
  },
] as const;

/** La page de départ de `r`, le bloc `r.id` portant `text`, de type `type`. */
function retyped(r: (typeof RETYPES)[number], text: string, type = "paragraph"): Shown[] {
  return [X, block(r.id, text, { type }), Z];
}

describe("changer le type d'un bloc", () => {
  for (const r of RETYPES) {
    describe(r.title, () => {
      test.each(ORDERS)(
        "changing_the_type_of_a_block_recreates_its_content_element ($label)",
        async (order) => {
          const s = await scene(r.seed, order);
          const before = elementIds(s.server);
          const wrote = written(s.mona, () => s.mona.editor.updateBlock(r.id, { type: "callout" }));
          // Le conteneur reste ; l'élément de contenu est supprimé, et un autre est créé, où le
          // texte est recopié.
          expect(wrote).toEqual({ deleted: [`paragraph de ${r.id}`], created: [`callout de ${r.id}`], renamed: {} });
          expect(counts(pending(s.mona))).toMatchObject({ made: r.made, deleted: r.deleted });
          await send(s, "mona");
          expectEverywhere(s, retyped(r, r.text, "callout"), { [r.id]: DEFAULT_CALLOUT });
          for (const [where, doc] of docs(s)) {
            const [was, now] = [before.get(r.id)!.split(" "), elementIds(doc).get(r.id)!.split(" ")];
            expect(now[0], `${where} : le même conteneur`).toBe(was[0]);
            expect(now[2], `${where} : un autre élément de contenu`).not.toBe(was[2]);
            expect(changedElements(doc, before).map((l) => l.split(" ")[0]), where).toEqual([r.id]);
          }
        },
      );

      test.each(ORDERS)("témoin : la frappe de Tom seule arrive partout ($label)", async (order) => {
        const s = await scene(r.seed, order);
        typeAtEnd(s.tom, r.id, TYPED);
        await send(s, "tom");
        expectEverywhere(s, retyped(r, `${r.text}${TYPED}`));
      });

      for (const { order, first, label } of PLAYS) {
        knownLimit(
          `changing_the_type_of_a_block_keeps_what_another_types (${label})`,
          "la frappe est dans l'élément de contenu que le changement de type supprime ; le correctif de la Task 13a ne joue pas à l'intérieur d'un bloc",
          { wanted: retyped(r, `${r.text}${TYPED}`, "callout"), lost: retyped(r, r.text, "callout") },
          async () => {
            const s = await scene(r.seed, order);
            s.mona.editor.updateBlock(r.id, { type: "callout" });
            typeAtEnd(s.tom, r.id, TYPED);
            requireLocallyDone(s, {
              mona: retyped(r, r.text, "callout"),
              tom: retyped(r, `${r.text}${TYPED}`),
            });
            await send(s, first);
            expectEverywhere(s, retyped(r, `${r.text}${TYPED}`, "callout"));
          },
        );
      }
    });
  }
});


// ---------------------------------------------------------------------------
// Poser une formule en ligne, en changer la formule (C2)
// ---------------------------------------------------------------------------

// Les pages se lisent ici avec les contenus en ligne dans le texte : une formule s'écrit `⟦latex⟧`
// (`shownInline`, `storedInline`). Ce qui est relu : la page chez Mona, chez Tom et au serveur, et
// les trois documents égaux ; le texte voisin d'une formule est un texte Yjs, pas un élément (`written`
// ne le voit pas) : `counts` dit ce qui est écrit.

/** Ce qui manque, avec les contenus en ligne : la page `wanted` partout, les documents égaux. */
function unmetInline(s: Scene, wanted: Shown[]): string[] {
  const lines: string[] = [];
  const views: [string, Shown[]][] = [
    ["chez Mona", shownInline(s.mona)],
    ["chez Tom", shownInline(s.tom)],
    ["au serveur", storedInline(s.server)],
  ];
  for (const [where, page] of views) {
    if (JSON.stringify(page) !== JSON.stringify(wanted)) lines.push(pageLine(where, page, wanted));
  }
  if (xml(s.tom.doc) !== xml(s.mona.doc)) lines.push("Mona et Tom n'ont pas le même document");
  if (xml(s.server) !== xml(s.mona.doc)) lines.push("le serveur n'a pas le document de Mona");
  return lines;
}

function expectEverywhereInline(s: Scene, wanted: Shown[]): void {
  const lines = unmetInline(s, wanted);
  if (lines.length > 0) throw new Unmet(lines.join("\n"));
}

/** Chacun montre, après son geste et avant l'échange, la page `local` dit (contenus en ligne compris). */
function requireLocallyDoneInline(s: Scene, local: { mona: Shown[]; tom: Shown[] }): void {
  const same = (p: Peer, page: Shown[]) => JSON.stringify(shownInline(p)) === JSON.stringify(page);
  if (s.quiet && same(s.mona, local.mona) && same(s.tom, local.tom)) return;
  throw new Missed(
    `précondition manquée : Mona ${line(shownInline(s.mona))} (attendu : ${line(local.mona)}) ; ` +
      `Tom ${line(shownInline(s.tom))} (attendu : ${line(local.tom)})`,
  );
}

/** La page XYZ, le bloc Y portant `text`. */
const withY = (text: string) => [X, block("y", text), Z];

/** Mona pose la formule `latex` dans Y, à `offset` caractères du début, comme « Équation en ligne ». */
function insertFormula(p: Peer, offset: number, latex: string): void {
  cursor(p, "y", offset);
  p.editor.insertInlineContent([{ type: "math", props: { latex } }]);
}

describe("poser une formule en ligne", () => {
  for (const { order, first, label } of PLAYS) {
    for (const [where, offset, wanted] of [
      ["au début", 0, "⟦x⟧Bloc Y de Tom"],
      ["à la fin", "Bloc Y".length, "Bloc Y de Tom⟦x⟧"],
    ] as const) {
      test(`inline_math_inserted_at_the_cursor_keeps_what_another_types, ${where} (${label})`, async () => {
        const s = await scene(XYZ, order);
        const before = elementIds(s.server);
        insertFormula(s.mona, offset, "x");
        // Tom tape à la fin de Y pendant que Mona pose sa formule.
        typeAtEnd(s.tom, "y", TYPED);
        const local = offset === 0 ? "⟦x⟧Bloc Y" : "Bloc Y⟦x⟧";
        requireLocallyDoneInline(s, { mona: withY(local), tom: withY(`Bloc Y${TYPED}`) });
        // Ce que le geste écrit : un élément de plus ; aucun caractère supprimé.
        expect(counts(pending(s.mona))).toMatchObject({ made: { "<math>": 1, "@latex": 1 }, deleted: 0 });
        await send(s, first);
        // À la fin, la formule et la frappe sont au même point : la frappe reste dans le texte, la
        // formule le suit, dans les deux ordres.
        expectEverywhereInline(s, withY(wanted));
        for (const [w, doc] of docs(s)) expect(changedElements(doc, before), w).toEqual([]);
      });
    }

    knownLimit(
      `inline_math_inserted_at_the_cursor_keeps_what_another_types, au milieu (${label})`,
      "le texte d'après la formule est supprimé et recréé dans un texte neuf : la frappe d'un autre, ancrée au bout du texte supprimé, reste avant la formule",
      { wanted: withY("Blo⟦x⟧c Y de Tom"), lost: withY("Blo de Tom⟦x⟧c Y") },
      async () => {
        const s = await scene(XYZ, order);
        insertFormula(s.mona, 3, "x");
        typeAtEnd(s.tom, "y", TYPED);
        requireLocallyDoneInline(s, { mona: withY("Blo⟦x⟧c Y"), tom: withY(`Bloc Y${TYPED}`) });
        await send(s, first);
        expectEverywhereInline(s, withY("Blo⟦x⟧c Y de Tom"));
      },
    );

    knownLimit(
      `two_inline_formulas_posed_at_once_in_a_text_keep_the_text_once (${label})`,
      "chacun supprime le texte d'après sa formule et le recrée dans un texte neuf : les deux textes neufs restent",
      {
        wanted: withY(order.mona < order.tom ? "Blo⟦a⟧⟦b⟧c Y" : "Blo⟦b⟧⟦a⟧c Y"),
        lost: withY(order.mona < order.tom ? "Blo⟦a⟧c Y⟦b⟧c Y" : "Blo⟦b⟧c Y⟦a⟧c Y"),
      },
      async () => {
        const s = await scene(XYZ, order);
        insertFormula(s.mona, 3, "a");
        insertFormula(s.tom, 3, "b");
        requireLocallyDoneInline(s, { mona: withY("Blo⟦a⟧c Y"), tom: withY("Blo⟦b⟧c Y") });
        await send(s, first);
        expectEverywhereInline(s, withY(order.mona < order.tom ? "Blo⟦a⟧⟦b⟧c Y" : "Blo⟦b⟧⟦a⟧c Y"));
      },
    );
  }
});

/** Y porte une formule entre deux textes. */
const WITH_FORMULA: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "paragraph", content: ["Avant ", { type: "math", props: { latex: "x" } }, " après"] },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];

describe("changer la formule d'un contenu en ligne", () => {
  test.each(PLAYS)("applying_the_latex_of_an_inline_formula_keeps_what_another_types ($label)", async ({ order, first }) => {
    const s = await scene(WITH_FORMULA, order);
    const before = elementIds(s.server);
    // « Terminer » : `updateInlineContent` de BlockNote, un `setNodeMarkup` du nœud de la formule.
    const wrote = written(s.mona, () =>
      s.mona.editor.transact((tr) => {
        let at = -1;
        tr.doc.descendants((node, pos) => {
          if (node.type.name === "math") at = pos;
        });
        tr.setNodeMarkup(at, undefined, { latex: "y" });
      }),
    );
    // Tom tape avant et après la formule.
    typeAt(s.tom, "y", 0, "Tom ");
    typeAtEnd(s.tom, "y", " Tom");
    requireLocallyDoneInline(s, { mona: withY("Avant ⟦y⟧ après"), tom: withY("Tom Avant ⟦x⟧ après Tom") });
    // Ce que le geste écrit : l'attribut de l'élément de la formule, récrit ; ni élément ni texte
    // créé ou supprimé.
    expect(wrote).toEqual({ deleted: [], created: [], renamed: {} });
    expect(counts(pending(s.mona))).toMatchObject({ made: { "attribut récrit": 1 }, deleted: 1 });
    await send(s, first);
    expectEverywhereInline(s, withY("Tom Avant ⟦y⟧ après Tom"));
    for (const [w, doc] of docs(s)) expect(changedElements(doc, before), w).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// Poser une mention (C4)
// ---------------------------------------------------------------------------

// Une mention s'écrit `⟦@libellé⟧`. Ce que posent `@` et `[[` : la mention, suivie d'une espace.

const PLAN = "0193a5b0-0000-7000-8000-000000000001";

/** Mona pose une mention de page dans Y, à `offset` caractères du début, comme le menu `@`. */
function insertMention(p: Peer, offset: number): void {
  cursor(p, "y", offset);
  p.editor.insertInlineContent([{ type: "mention", props: { kind: "page", id: PLAN, label: "Plan" } }, " "]);
}

/** Le texte de Y dans chacune des trois vues. */
function textsOfY(s: Scene): [string, string][] {
  const y = (page: Shown[]) => page.find((b) => b.id === "y")?.text ?? "(absent)";
  return [
    ["chez Mona", y(shownInline(s.mona))],
    ["chez Tom", y(shownInline(s.tom))],
    ["au serveur", y(storedInline(s.server))],
  ];
}

describe("poser une mention", () => {
  for (const { order, first, label } of PLAYS) {
    test(`mention_inserted_at_the_end_keeps_what_another_types (${label})`, async () => {
      const s = await scene(XYZ, order);
      const before = elementIds(s.server);
      insertMention(s.mona, "Bloc Y".length);
      typeAtEnd(s.tom, "y", TYPED);
      requireLocallyDoneInline(s, { mona: withY("Bloc Y⟦@Plan⟧ "), tom: withY(`Bloc Y${TYPED}`) });
      // Ce que le geste écrit : l'élément de la mention et ses trois attributs ; aucun caractère supprimé.
      expect(counts(pending(s.mona))).toMatchObject({
        made: { "<mention>": 1, "@kind": 1, "@id": 1, "@label": 1 },
        deleted: 0,
      });
      await send(s, first);
      // La mention et la frappe sont au même point : la frappe reste dans le texte, la mention et
      // son espace le suivent, dans les deux ordres.
      expectEverywhereInline(s, withY("Bloc Y de Tom⟦@Plan⟧ "));
      for (const [w, doc] of docs(s)) expect(changedElements(doc, before), w).toEqual([]);
    });

    // Exigé : la frappe de Tom n'est ni perdue ni doublée, le reste du texte non plus, et les trois
    // documents sont identiques. Où elle atterrit est la limite, mesurée à part, juste après.
    test(`mention_inserted_in_the_middle_keeps_what_another_types_after_it (${label})`, async () => {
      const s = await scene(XYZ, order);
      insertMention(s.mona, 3);
      typeAtEnd(s.tom, "y", TYPED);
      requireLocallyDoneInline(s, { mona: withY("Blo⟦@Plan⟧ c Y"), tom: withY(`Bloc Y${TYPED}`) });
      await send(s, first);
      for (const [where, text] of textsOfY(s)) {
        expect(text.split(TYPED).length - 1, `${where} : la frappe de Tom, une fois (« ${text} »)`).toBe(1);
        expect(text.replace(TYPED, ""), `${where} : le reste du texte`).toBe("Blo⟦@Plan⟧ c Y");
      }
      expect(xml(s.tom.doc)).toBe(xml(s.mona.doc));
      expect(xml(s.server)).toBe(xml(s.mona.doc));
    });

    knownLimit(
      `mention_inserted_in_the_middle_keeps_what_another_types_after_it, à sa place (${label})`,
      "le texte d'après la mention est supprimé et recréé dans un texte neuf : la frappe d'un autre, ancrée au bout du texte supprimé, reste avant la mention",
      { wanted: withY("Blo⟦@Plan⟧ c Y de Tom"), lost: withY("Blo de Tom⟦@Plan⟧ c Y") },
      async () => {
        const s = await scene(XYZ, order);
        insertMention(s.mona, 3);
        typeAtEnd(s.tom, "y", TYPED);
        requireLocallyDoneInline(s, { mona: withY("Blo⟦@Plan⟧ c Y"), tom: withY(`Bloc Y${TYPED}`) });
        await send(s, first);
        expectEverywhereInline(s, withY("Blo⟦@Plan⟧ c Y de Tom"));
      },
    );
  }
});

// ---------------------------------------------------------------------------
// Dupliquer un bloc (C5)
// ---------------------------------------------------------------------------

// Ctrl+D et « Dupliquer » du menu de la poignée insèrent, après le bloc, sa copie sans aucun id
// (`duplicateBlocks`, `duplicate.ts`) : BlockNote en tire de neufs, à chaque profondeur. La première
// mesure joue le vrai geste. La seconde dit ce que porterait le document si la copie gardait les ids
// de l'original, ce que `duplicateBlocks` ne doit jamais faire : la même chose avec ou sans le
// correctif de la Task 13a, qui rend la main au code d'origine dès qu'un id se répète (R223).

describe("dupliquer un bloc", () => {
  test.each(PLAYS)("duplicating_touches_no_existing_element ($label)", async ({ order, first }) => {
    const s = await scene(PARENTED, order);
    const before = elementIds(s.server);
    let ids: string[] = [];
    const wrote = written(s.mona, () => {
      ids = duplicateBlocks(s.mona.editor, ["x"]);
    });
    expect(ids, "une copie, de X").toHaveLength(1);
    // Tom tape dans l'original (X, et son enfant X1) et dans le bloc d'après (Y).
    typeAtEnd(s.tom, "x", TYPED);
    typeAtEnd(s.tom, "x1", TYPED);
    typeAtEnd(s.tom, "y", TYPED);
    const [copy] = ids;
    const child = s.mona.editor.getBlock(copy)?.children[0]?.id ?? "(aucun enfant)";
    const copied = block(copy, "Bloc X", { children: [block(child, "Enfant X1")] });
    requireLocallyDone(s, { mona: [X_AS_SEEDED, copied, Y_, Z], tom: [X_TYPED, Y_TYPED, Z] });

    // Ce que le geste écrit : la copie et son enfant, des éléments neufs sous des ids neufs ; rien de
    // supprimé, aucun id réattribué.
    expect(wrote).toEqual({
      deleted: [],
      created: [
        `blockContainer ${copy}`,
        `paragraph de ${copy}`,
        `blockGroup de ${copy}`,
        `blockContainer ${child}`,
        `paragraph de ${child}`,
      ],
      renamed: {},
    });
    expect(counts(pending(s.mona))).toMatchObject({ made: { "<blockContainer>": 2, "@id": 2 }, deleted: 0 });

    await send(s, first);
    // Rien de perdu : la frappe de Tom est dans l'original, pas dans la copie, qui porte ce que Mona
    // a copié ; les trois documents sont égaux.
    expectEverywhere(s, [X_TYPED, copied, Y_TYPED, Z]);
    for (const [where, doc] of docs(s)) {
      // Aucun bloc qui existait ne change d'élément ; la copie et son enfant, une fois chacun.
      expect(changedElements(doc, before), where).toEqual([]);
      expect(newIds(doc, before), where).toEqual([copy, child].sort());
      expect([...elementIds(doc).values()].filter((v) => v.includes(" ; ")), where).toEqual([]);
    }
  });

  test.each(ORDERS)("a_copy_that_kept_its_id_is_not_a_copy ($label)", async (order) => {
    const s = await scene(PARENTED, order);
    // La copie de X, son enfant compris, insérée après X avec les ids de l'original.
    const wrote = written(s.mona, () =>
      s.mona.editor.insertBlocks(
        [
          {
            id: "x",
            type: "paragraph",
            content: "Bloc X",
            children: [{ id: "x1", type: "paragraph", content: "Enfant X1" }],
          },
        ],
        "x",
        "after",
      ),
    );
    // Rien n'est supprimé ni renommé : le document de Mona porte deux conteneurs x, et deux x1.
    expect(wrote).toEqual({
      deleted: [],
      created: ["blockContainer x", "paragraph de x", "blockGroup de x", "blockContainer x1", "paragraph de x1"],
      renamed: {},
    });
    expect(stored(s.mona.doc)).toEqual([X_AS_SEEDED, X_AS_SEEDED, Y_, Z]);
    // Mona, qui l'a faite, voit deux X du même id.
    expect(shown(s.mona)).toEqual([X_AS_SEEDED, X_AS_SEEDED, Y_, Z]);

    await send(s, "mona");
    // Partout, deux conteneurs de l'id x et deux de l'id x1 : le serveur de Mementum supprime le
    // second de chaque, donc la copie (`dedupe_blocks`, `crates/mementum-crdt/src/dedupe.rs`). Le
    // serveur du banc est un document sans dédoublonnage.
    for (const [where, doc] of docs(s)) {
      expect(stored(doc), where).toEqual([X_AS_SEEDED, X_AS_SEEDED, Y_, Z]);
      expect(elementIds(doc).get("x")?.split(" ; "), where).toHaveLength(2);
      expect(elementIds(doc).get("x1")?.split(" ; "), where).toHaveLength(2);
    }
    // Tom, qui la reçoit, montre les deux X sous des ids neufs, l'original compris, sans encore les
    // écrire.
    const seen = shown(s.tom);
    expect(seen.map((b) => b.text)).toEqual(["Bloc X", "Bloc X", "Bloc Y", "Bloc Z"]);
    expect(seen.slice(0, 2).map((b) => b.id)).not.toContain("x");
    expect(s.tom.writes).toEqual([]);
    // À sa première écriture (une frappe dans Y), Tom écrit ces ids neufs : l'original perd le sien,
    // et ce qui le nommait (un fil de commentaires) s'en détache.
    typeAtEnd(s.tom, "y", TYPED);
    const rewritten = stored(s.tom.doc);
    expect(rewritten.map((b) => b.id)).toEqual([...seen.slice(0, 2).map((b) => b.id), "y", "z"]);
    expect(repeatedIds(rewritten)).toEqual([]);
  });
});
