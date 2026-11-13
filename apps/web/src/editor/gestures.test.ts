// Ce que les gestes de la partie B du lot 3 écrivent dans le document partagé, et ce que devient
// une frappe faite au même instant par quelqu'un d'autre (plan de la partie B, « Global
// Constraints », « Gestes de l'éditeur »). Toute tâche de la partie B qui ajoute un geste commence
// ici, par le test qui le relit dans le document : insérer des blocs (C1a ; dupliquer, C5), changer
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
  drag,
  duo,
  elementIds,
  exchange,
  line,
  PARENTED,
  repeatedIds,
  shown,
  stored,
  storedProps,
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
