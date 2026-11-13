// Ce que chaque déplacement de bloc écrit dans le document partagé, et ce que devient une frappe
// faite en même temps par quelqu'un d'autre (limite R39, Task 12 ; la Task 13 doit la lever).
//
// Deux vrais éditeurs BlockNote 0.57, ceux de Mona et de Tom, liés à deux `Y.Doc` aux
// identifiants de client fixes (`test/editors.ts`). Chacun fait son geste depuis le même état ;
// leurs mises à jour s'échangent ensuite à la main, et passent par un troisième document, celui
// du « serveur », dans un ordre donné. L'issue d'un conflit dépend de l'ordre des identifiants
// de client : chaque scénario est joué deux fois, Mona avant Tom puis Tom avant Mona (`ORDERS`).
//
// Chaque scénario (`SCENARIOS`, S1 à S11 : les noms du registre du lot 3, « Task 12 ») attend le
// comportement voulu : les blocs dans l'ordre, chaque id une seule fois, chaque texte dans son
// bloc, types et enfants compris, rien en double, Mona, Tom et le serveur d'accord. Ceux qui
// échouent aujourd'hui portent leur raison (`loss`) et sont marqués `test.fails` : la suite reste
// verte, et le jour où l'un passe, elle le dit. `MEMENTUM_SHOW_R39=1` les lance comme des tests
// ordinaires, pour voir leur échec. Seule l'exigence du scénario peut être cet échec : une
// précondition manquée (un geste qui n'a pas fait chez son auteur ce que le scénario dit) ou une
// exception d'un éditeur lève, et ne passe jamais pour la perte attendue (`outcomeOf`). Chaque
// geste a son témoin, vert : seul, il fait ce qu'on attend. Une dernière partie relit ce que
// chaque geste, fait seul, écrit dans le document : les éléments Yjs gardés (et l'id qu'ils
// portent ensuite), supprimés, créés.
//
// Une correction faite par le serveur (R138) ne rendrait pas ces tests verts : il n'y a pas de
// serveur ici. Ils disent ce que les clients écrivent et ce que donne leur fusion ; ce que le
// serveur en fait se vérifie dans `crates/mementum-sync/tests/moves.rs`, avec des octets que cette
// suite produit (fin du fichier), et dans un navigateur, dans `e2e/block-move.spec.ts`.
import { describe, expect, test } from "vitest";
import {
  A,
  block,
  drag,
  duo,
  exchange,
  keyboardDown,
  line,
  outgoing,
  PARENTED,
  peer,
  repeatedIds,
  shiftTab,
  shown,
  stored,
  tab,
  TITLE_Y,
  TITLED,
  typeAtEnd,
  W,
  W_FIRST,
  W_PARENT,
  written,
  wWithChild,
  X,
  xml,
  xWithChild,
  XYZ,
  XYZA,
  XYZW,
  Y_,
  Z,
  type Peer,
  type Seed,
  type Shown,
} from "../test/editors";
import { nodeModule, type Fs, type Path, type Url } from "../test/node";

/** Ce que Tom tape. */
const TYPED = " de Tom";

/**
 * Les deux ordres des identifiants de client de Mona et de Tom (la page de départ est écrite par
 * l'identifiant 1). `dir` : le dossier des octets de cet ordre, pour le test de l'acteur.
 */
const ORDERS = [
  { label: "Mona avant Tom", dir: "mona-first", mona: 2, tom: 3 },
  { label: "Tom avant Mona", dir: "tom-first", mona: 3, tom: 2 },
] as const;
type Order = (typeof ORDERS)[number];

// ---------------------------------------------------------------------------
// Les pages de départ
// ---------------------------------------------------------------------------

// Les autres pages (« Bloc X, Bloc Y, Bloc Z » et ses variantes) sont dans `test/editors.ts`.

/** Des textes presque sans rien en commun : le diff de y-prosemirror récrit presque tout. */
const CHORES: Seed = [
  { id: "x", type: "paragraph", content: "Acheter du pain" },
  { id: "y", type: "paragraph", content: "Rappeler Marc" },
  { id: "z", type: "paragraph", content: "Arroser les plantes" },
];
const CHORE_X = block("x", "Acheter du pain");
const CHORE_Y = block("y", "Rappeler Marc");
const CHORE_Z = block("z", "Arroser les plantes");

// ---------------------------------------------------------------------------
// Une scène, ses préconditions, son exigence
// ---------------------------------------------------------------------------

/**
 * Une scène : la page de départ, Mona et Tom qui l'ont ouverte, leurs identifiants de client
 * dans l'ordre `order`, et le document du serveur. `quiet` : ni Mona ni Tom n'a rien écrit en
 * ouvrant la page.
 */
function scene(blocks: Seed, order: Order) {
  return duo(blocks, order.mona, order.tom);
}

type Scene = Awaited<ReturnType<typeof scene>>;

/** Une précondition manquée : le test prouverait autre chose que ce qu'il dit. */
class Missed extends Error {}

/** L'exigence d'un scénario n'est pas tenue : une ligne par manque. */
class Unmet extends Error {}

/**
 * Les gestes sont faits, chacun vérifié chez son auteur avant l'échange (`local`), et personne
 * n'a rien écrit en ouvrant la page. Sinon, lève `Missed` : dans un test ordinaire comme dans un
 * `test.fails`, une précondition manquée n'est jamais un succès, ni la perte attendue.
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

/**
 * Ce qui manque à la page voulue, après l'échange, une ligne par manque : chez Mona et chez Tom
 * (ce que montrent leurs éditeurs) et au serveur (son document, relu sans éditeur), chaque id
 * une seule fois et la page entière, types et enfants compris ; puis les trois documents
 * identiques. `wanted` : une page, ou les pages admises quand deux issues sont justes.
 */
function unmet(s: Scene, wanted: Shown[] | Shown[][]): string[] {
  const pages = Array.isArray(wanted[0]) ? (wanted as Shown[][]) : [wanted as Shown[]];
  const admitted = pages.map((page) => JSON.stringify(page));
  const views: [string, Shown[]][] = [
    ["chez Mona", shown(s.mona)],
    ["chez Tom", shown(s.tom)],
    ["au serveur", stored(s.server)],
  ];
  const lines: string[] = [];
  for (const [where, page] of views) {
    const repeated = repeatedIds(page);
    if (repeated.length > 0) {
      lines.push(`${where} : chaque id une seule fois (en double : ${repeated.join(", ")} ; vu : ${line(page)})`);
    }
    if (!admitted.includes(JSON.stringify(page))) {
      lines.push(
        `${where} : la page voulue (vu : ${line(page)} ; voulu : ${pages.map(line).join(" ou ")})`,
      );
    }
  }
  if (xml(s.tom.doc) !== xml(s.mona.doc)) lines.push("Mona et Tom n'ont pas le même document");
  if (xml(s.server) !== xml(s.mona.doc)) lines.push("le serveur n'a pas le document de Mona");
  return lines;
}

/** La page voulue partout (`unmet`) ; sinon, lève `Unmet`, avec tout ce qui manque. */
function expectEverywhere(s: Scene, wanted: Shown[] | Shown[][]): void {
  const lines = unmet(s, wanted);
  if (lines.length > 0) throw new Unmet(lines.join("\n"));
}

/** Un témoin : les deux ouvertures n'ont rien écrit. */
function expectQuiet(s: Scene): void {
  expect(s.quiet, "Mona et Tom n'écrivent rien en ouvrant la page").toBe(true);
}

/**
 * Ce que donne un scénario : `juste` (l'exigence est tenue), `perte` (elle ne l'est pas : `Unmet`),
 * ou `accident` (toute autre exception : précondition manquée, erreur d'un éditeur, échange qui
 * ne finit pas).
 */
type Outcome =
  | { kind: "juste" }
  | { kind: "perte"; error: Unmet }
  | { kind: "accident"; error: unknown };

async function outcomeOf(body: () => Promise<void>): Promise<Outcome> {
  try {
    await body();
    return { kind: "juste" };
  } catch (error) {
    return error instanceof Unmet ? { kind: "perte", error } : { kind: "accident", error };
  }
}

/**
 * Un scénario dont l'exigence n'est pas tenue aujourd'hui, pour la raison `reason` : `test.fails`,
 * sauf avec `MEMENTUM_SHOW_R39=1`, qui le lance comme un test ordinaire et montre son échec.
 * `test.fails` tient toute exception pour l'échec attendu : seule l'exigence du scénario (`Unmet`)
 * lui est donc rendue. Un accident est écrit au journal, et le test rend la main sans échouer, ce
 * que `test.fails` signale en rouge, comme un scénario devenu juste (à repasser en `test`).
 */
function knownLoss(name: string, reason: string, body: () => Promise<void>): void {
  const title = `${name} [perte connue : ${reason}]`;
  if (import.meta.env.MEMENTUM_SHOW_R39) {
    test(title, body);
    return;
  }
  test.fails(title, async () => {
    const outcome = await outcomeOf(body);
    if (outcome.kind === "perte") throw outcome.error;
    if (outcome.kind === "accident") {
      console.error(`${name} : pas la perte attendue, un accident :`, outcome.error);
    }
  });
}

// ---------------------------------------------------------------------------
// Les scénarios
// ---------------------------------------------------------------------------

/** Qui envoie le premier ce qu'il a écrit, au serveur et à l'autre. */
type First = "mona" | "tom";

type Scenario = {
  /** Son nom au registre (`s1` à `s11`), et celui de son dossier d'octets. */
  key: string;
  title: string;
  seed: Seed;
  /** Le geste de Mona, puis celui de Tom, chacun chez soi, depuis la page de départ. */
  mona: (p: Peer) => void;
  tom: (p: Peer) => void;
  /** Ce que chacun montre après son geste, avant l'échange. */
  local: { mona: Shown[]; tom: Shown[] };
  /** La page voulue, ou les pages admises quand deux issues sont justes. */
  wanted: Shown[] | Shown[][];
  /** Les ordres d'envoi joués (par défaut, Mona d'abord). */
  firsts?: First[];
  /**
   * Pourquoi l'exigence n'est pas tenue aujourd'hui ; par ordre d'identifiants quand l'issue en
   * dépend (un ordre absent passe). Absent : le scénario passe.
   */
  loss?: string | Partial<Record<Order["dir"], string>>;
};

/** Limite R39 : ce que fait y-prosemirror 1.3.7 d'un déplacement, que la Task 13 doit lever. */
const R39 = "limite R39, levée par la Task 13";

const typedIn = {
  x: block("x", `Bloc X${TYPED}`),
  y: block("y", `Bloc Y${TYPED}`),
  z: block("z", `Bloc Z${TYPED}`),
};

const SCENARIOS: Scenario[] = [
  {
    key: "s1",
    title: "Tom tape dans X pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [Y_, Z, X], tom: [typedIn.x, Y_, Z] },
    wanted: [Y_, Z, typedIn.x],
    firsts: ["mona", "tom"],
    loss: R39,
  },
  {
    key: "s2",
    title: "Y est un titre ; Tom tape dans X pendant que Mona glisse X sous Z",
    seed: TITLED,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [TITLE_Y, Z, X], tom: [typedIn.x, TITLE_Y, Z] },
    wanted: [TITLE_Y, Z, typedIn.x],
    loss: R39,
  },
  {
    key: "s3",
    title: "Tom tape dans l'enfant de X pendant que Mona glisse X sous Z",
    seed: PARENTED,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "x1", TYPED),
    local: {
      mona: [Y_, Z, xWithChild("Enfant X1")],
      tom: [xWithChild(`Enfant X1${TYPED}`), Y_, Z],
    },
    wanted: [Y_, Z, xWithChild(`Enfant X1${TYPED}`)],
    loss: R39,
  },
  {
    key: "s4",
    title: "Tom tape dans Y, que personne ne déplace, pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "y", TYPED),
    local: { mona: [Y_, Z, X], tom: [X, typedIn.y, Z] },
    wanted: [typedIn.y, Z, X],
    loss: R39,
  },
  {
    key: "s5",
    title: "Tom tape dans Z pendant que Mona glisse W tout en haut",
    seed: XYZW,
    mona: (p) => drag(p, "w", { above: "x" }),
    tom: (p) => typeAtEnd(p, "z", TYPED),
    local: { mona: [W, X, Y_, Z], tom: [X, Y_, typedIn.z, W] },
    wanted: [W, X, Y_, typedIn.z],
    loss: R39,
  },
  {
    key: "s6",
    title: "sans aucune frappe, Mona glisse X sous Z, Tom glisse X sous Y",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => drag(p, "x", { under: "y" }),
    local: { mona: [Y_, Z, X], tom: [Y_, X, Z] },
    // L'une ou l'autre place est juste ; pas les deux, et aucun texte ne change.
    wanted: [
      [Y_, Z, X],
      [Y_, X, Z],
    ],
    loss: R39,
  },
  {
    key: "s7",
    title: "Tom tape dans X pendant que Mona le descend d'une place au clavier",
    seed: XYZ,
    mona: (p) => keyboardDown(p, "x"),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [Y_, X, Z], tom: [typedIn.x, Y_, Z] },
    wanted: [Y_, typedIn.x, Z],
    loss: R39,
  },
  {
    key: "s8",
    title: "textes différents ; Tom tape dans X pendant que Mona glisse X sous Z",
    seed: CHORES,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: {
      mona: [CHORE_Y, CHORE_Z, CHORE_X],
      tom: [block("x", `Acheter du pain${TYPED}`), CHORE_Y, CHORE_Z],
    },
    wanted: [CHORE_Y, CHORE_Z, block("x", `Acheter du pain${TYPED}`)],
    loss: R39,
  },
  {
    key: "s9",
    title: "Tom tape dans X pendant que Mona en fait l'enfant de W (Tab)",
    seed: W_FIRST,
    mona: (p) => tab(p, "x"),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [wWithChild("Bloc X"), Y_], tom: [W, typedIn.x, Y_] },
    wanted: [wWithChild(`Bloc X${TYPED}`), Y_],
    loss: R39,
  },
  {
    key: "s10",
    title: "Tom tape dans X pendant que Mona le remonte d'un niveau (Maj+Tab)",
    seed: W_PARENT,
    mona: (p) => shiftTab(p, "x"),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [W, X, Y_], tom: [wWithChild(`Bloc X${TYPED}`), Y_] },
    wanted: [W, typedIn.x, Y_],
    loss: R39,
  },
  {
    key: "s11",
    title: "sans aucune frappe, Mona glisse X sous Z, Tom glisse Y sous A",
    seed: XYZA,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => drag(p, "y", { under: "a" }),
    local: { mona: [Y_, Z, X, A], tom: [X, Z, A, Y_] },
    wanted: [Z, X, A, Y_],
    loss: R39,
  },
];

/**
 * Joue `sc` : la scène, les deux gestes, leurs préconditions, puis l'échange, `first` envoyant le
 * premier. Rend la scène, à comparer à la page voulue.
 */
async function play(sc: Scenario, order: Order, first: First): Promise<Scene> {
  const s = await scene(sc.seed, order);
  sc.mona(s.mona);
  sc.tom(s.tom);
  requireLocallyDone(s, sc.local);
  await exchange(s.server, first === "mona" ? [s.mona, s.tom] : [s.tom, s.mona]);
  return s;
}

/** Pourquoi `sc` n'est pas tenu dans l'ordre `order` ; `undefined` s'il passe. */
function lossOf(sc: Scenario, order: Order): string | undefined {
  return typeof sc.loss === "object" ? sc.loss[order.dir] : sc.loss;
}

const FIRST_LABEL: Record<First, string> = {
  mona: "Mona envoie d'abord",
  tom: "Tom envoie d'abord",
};

for (const order of ORDERS) {
  describe(`${order.label} (identifiants de client ${order.mona} et ${order.tom})`, () => {
    describe("témoins : chaque geste, fait seul", () => {
      test("le glisser seul met X sous Z, partout", async () => {
        const s = await scene(XYZ, order);
        expectQuiet(s);
        drag(s.mona, "x", { under: "z" });
        expect(shown(s.mona)).toEqual([Y_, Z, X]);
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [Y_, Z, X]);
      });

      test("le glisser de Tom seul met X sous Y, partout", async () => {
        const s = await scene(XYZ, order);
        expectQuiet(s);
        drag(s.tom, "x", { under: "y" });
        expect(shown(s.tom)).toEqual([Y_, X, Z]);
        await exchange(s.server, [s.tom, s.mona]);
        expectEverywhere(s, [Y_, X, Z]);
      });

      test("la frappe seule arrive partout", async () => {
        const s = await scene(XYZ, order);
        expectQuiet(s);
        typeAtEnd(s.tom, "x", TYPED);
        await exchange(s.server, [s.tom, s.mona]);
        expectEverywhere(s, [typedIn.x, Y_, Z]);
      });

      test("textes différents, le glisser seul", async () => {
        const s = await scene(CHORES, order);
        expectQuiet(s);
        drag(s.mona, "x", { under: "z" });
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [CHORE_Y, CHORE_Z, CHORE_X]);
      });

      test("le glisser seul met W en tête, partout", async () => {
        const s = await scene(XYZW, order);
        expectQuiet(s);
        drag(s.mona, "w", { above: "x" });
        expect(shown(s.mona)).toEqual([W, X, Y_, Z]);
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [W, X, Y_, Z]);
      });

      test("par-dessus un titre, le glisser seul met X sous Z, partout, le titre gardé", async () => {
        const s = await scene(TITLED, order);
        expectQuiet(s);
        drag(s.mona, "x", { under: "z" });
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [TITLE_Y, Z, X]);
      });

      test("le glisser seul met X et son enfant sous Z, partout", async () => {
        const s = await scene(PARENTED, order);
        expectQuiet(s);
        drag(s.mona, "x", { under: "z" });
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [Y_, Z, xWithChild("Enfant X1")]);
      });

      test("la frappe seule dans l'enfant arrive partout", async () => {
        const s = await scene(PARENTED, order);
        expectQuiet(s);
        typeAtEnd(s.tom, "x1", TYPED);
        await exchange(s.server, [s.tom, s.mona]);
        expectEverywhere(s, [xWithChild(`Enfant X1${TYPED}`), Y_, Z]);
      });

      test("au clavier, X descend d'une place, partout", async () => {
        const s = await scene(XYZ, order);
        expectQuiet(s);
        keyboardDown(s.mona, "x");
        expect(shown(s.mona)).toEqual([Y_, X, Z]);
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [Y_, X, Z]);
      });

      test("Tab seul fait de X l'enfant de W, partout", async () => {
        const s = await scene(W_FIRST, order);
        expectQuiet(s);
        tab(s.mona, "x");
        expect(shown(s.mona)).toEqual([wWithChild("Bloc X"), Y_]);
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [wWithChild("Bloc X"), Y_]);
      });

      test("Maj+Tab seul remonte X au niveau de W, partout", async () => {
        const s = await scene(W_PARENT, order);
        expectQuiet(s);
        shiftTab(s.mona, "x");
        expect(shown(s.mona)).toEqual([W, X, Y_]);
        await exchange(s.server, [s.mona, s.tom]);
        expectEverywhere(s, [W, X, Y_]);
      });

      test("le glisser de Tom seul met Y sous A, partout", async () => {
        const s = await scene(XYZA, order);
        expectQuiet(s);
        drag(s.tom, "y", { under: "a" });
        await exchange(s.server, [s.tom, s.mona]);
        expectEverywhere(s, [X, Z, A, Y_]);
      });
    });

    describe("scénarios : deux gestes en même temps", () => {
      for (const sc of SCENARIOS) {
        for (const first of sc.firsts ?? (["mona"] as const)) {
          const name = `${sc.key.toUpperCase()} : ${sc.title} ; ${FIRST_LABEL[first]}`;
          const body = async () => expectEverywhere(await play(sc, order, first), sc.wanted);
          const reason = lossOf(sc, order);
          if (reason === undefined) test(name, body);
          else knownLoss(name, reason, body);
        }
      }
    });
  });
}

// ---------------------------------------------------------------------------
// La cible elle-même
// ---------------------------------------------------------------------------

// Ce sur quoi reposent les scénarios : une précondition manquée ou une exception ne passent
// jamais pour la perte attendue, et la comparaison voit les types et les enfants.
describe("la cible : ce qu'un scénario peut donner", () => {
  const order = ORDERS[0];
  const s4 = SCENARIOS.find((sc) => sc.key === "s4")!;

  test("une précondition manquée est un accident, pas la perte attendue", async () => {
    // Le scénario dit que Mona a mis X sous Z ; elle ne fait rien.
    const idle: Scenario = { ...s4, mona: () => {} };
    const outcome = await outcomeOf(async () => expectEverywhere(await play(idle, order, "mona"), idle.wanted));
    expect(outcome.kind).toBe("accident");
    expect((outcome as { error: unknown }).error).toBeInstanceOf(Missed);
  });

  test("une exception d'un geste est un accident, pas la perte attendue", async () => {
    const broken: Scenario = { ...s4, tom: (p) => typeAtEnd(p, "aucun", TYPED) };
    const outcome = await outcomeOf(async () => expectEverywhere(await play(broken, order, "mona"), broken.wanted));
    expect(outcome.kind).toBe("accident");
    expect(String((outcome as { error: unknown }).error)).toContain("aucun bloc aucun");
  });

  test("une exigence tenue est dite juste, une exigence manquée est dite perte", async () => {
    // Un seul geste : la frappe de Tom dans Y arrive partout.
    const typing: Scenario = {
      ...s4,
      mona: () => {},
      local: { mona: [X, Y_, Z], tom: [X, typedIn.y, Z] },
      wanted: [X, typedIn.y, Z],
    };
    const met = await outcomeOf(async () => expectEverywhere(await play(typing, order, "mona"), typing.wanted));
    expect(met.kind).toBe("juste");
    const wrong = [X, Y_, Z];
    const lost = await outcomeOf(async () => expectEverywhere(await play(typing, order, "mona"), wrong));
    expect(lost.kind).toBe("perte");
    expect((lost as { error: Unmet }).error.message).toContain("chez Mona : la page voulue");
    expect((lost as { error: Unmet }).error.message).toContain("au serveur : la page voulue");
  });

  test("la comparaison voit le type d'un bloc et ses enfants, chez chacun et au serveur", async () => {
    const titled = await scene(TITLED, order);
    expect(unmet(titled, [X, TITLE_Y, Z])).toEqual([]);
    // Le même texte dans un paragraphe n'est pas la page.
    const flat = unmet(titled, [X, block("y", "Titre Y"), Z]);
    expect(flat.filter((l) => l.includes("la page voulue"))).toHaveLength(3);

    const parented = await scene(PARENTED, order);
    expect(unmet(parented, [xWithChild("Enfant X1"), Y_, Z])).toEqual([]);
    // Ni sans l'enfant, ni avec un autre texte dans l'enfant.
    expect(unmet(parented, [X, Y_, Z]).filter((l) => l.includes("la page voulue"))).toHaveLength(3);
    expect(
      unmet(parented, [xWithChild("Enfant"), Y_, Z]).filter((l) => l.includes("la page voulue")),
    ).toHaveLength(3);
  });
});

// ---------------------------------------------------------------------------
// Ce que chaque geste écrit
// ---------------------------------------------------------------------------

// Ce qu'écrivent y-prosemirror 1.3.7 et BlockNote 0.57 pour chaque geste, fait seul : ce sur
// quoi repose le tableau du registre (Task 12). Une mise à jour de l'une ou de l'autre peut
// changer ces formes ; ces tests le diront.
describe("ce que chaque geste écrit dans le document", () => {
  const order = ORDERS[0];

  test("glisser au même niveau : les blocs de la plage changent d'id sur place, rien n'est supprimé", async () => {
    const s = await scene(XYZ, order);
    expect(written(s.mona, () => drag(s.mona, "x", { under: "z" }))).toEqual({
      deleted: [],
      created: [],
      renamed: { x: "y", y: "z", z: "x" },
    });
  });

  test("glisser de plusieurs places vers le haut : tout bloc entre les deux places change d'id", async () => {
    const s = await scene(XYZW, order);
    expect(written(s.mona, () => drag(s.mona, "w", { above: "x" }))).toEqual({
      deleted: [],
      created: [],
      renamed: { x: "w", y: "x", z: "y", w: "z" },
    });
  });

  test("déplacer au clavier : la même récriture sur place que le glisser", async () => {
    const s = await scene(XYZ, order);
    expect(written(s.mona, () => keyboardDown(s.mona, "x"))).toEqual({
      deleted: [],
      created: [],
      renamed: { x: "y", y: "x" },
    });
  });

  test("glisser par-dessus un autre type : le contenu d'un autre type est supprimé et recréé", async () => {
    const s = await scene(TITLED, order);
    expect(written(s.mona, () => drag(s.mona, "x", { under: "z" }))).toEqual({
      deleted: ["paragraph de x", "heading de y"],
      created: ["heading de y", "paragraph de z"],
      renamed: { x: "y", y: "z", z: "x" },
    });
  });

  test("glisser un bloc qui a des enfants : les enfants sont copiés sous un autre élément", async () => {
    const s = await scene(PARENTED, order);
    expect(written(s.mona, () => drag(s.mona, "x", { under: "z" }))).toEqual({
      deleted: ["blockGroup de x", "blockContainer x1", "paragraph de x1"],
      created: ["blockGroup de x", "blockContainer x1", "paragraph de x1"],
      renamed: { x: "y", y: "z", z: "x" },
    });
  });

  test("Tab : une copie au même id sous le bloc d'au-dessus, l'original supprimé", async () => {
    const s = await scene(W_FIRST, order);
    expect(written(s.mona, () => tab(s.mona, "x"))).toEqual({
      deleted: ["blockContainer x", "paragraph de x"],
      created: ["blockGroup de w", "blockContainer x", "paragraph de x"],
      renamed: {},
    });
  });

  test("Maj+Tab : une copie au même id un niveau plus haut, l'original supprimé", async () => {
    const s = await scene(W_PARENT, order);
    expect(written(s.mona, () => shiftTab(s.mona, "x"))).toEqual({
      deleted: ["blockGroup de w", "blockContainer x", "paragraph de x"],
      created: ["blockContainer x", "paragraph de x"],
      renamed: {},
    });
  });
});

// ---------------------------------------------------------------------------
// Les octets du test de l'acteur
// ---------------------------------------------------------------------------

/** Où `crates/mementum-sync/tests/moves.rs` lit ces octets, depuis la racine du dépôt. */
const FIXTURES = "crates/mementum-sync/tests/fixtures/moves";

/**
 * Ce que rejoue le test de l'acteur, écrit par y-prosemirror depuis la même page de départ
 * (« Bloc X, Bloc Y, Bloc Z », identifiant de client 1) : le glisser de X sous Z fait par Mona
 * (identifiant 2), et la frappe de Tom (identifiant 3) à la fin de X, ou de Y.
 */
async function fixtureBytes(): Promise<Record<string, Uint8Array>> {
  const order = ORDERS[0];
  const s = await scene(XYZ, order);
  const tomInY = await peer("Tom", order.tom, s.base);
  expect(s.quiet && tomInY.writes.length === 0, "rien d'écrit à l'ouverture").toBe(true);
  drag(s.mona, "x", { under: "z" });
  typeAtEnd(s.tom, "x", TYPED);
  typeAtEnd(tomInY, "y", TYPED);
  return {
    "base.bin": s.base,
    "mona-drags-x-under-z.bin": outgoing(s.mona)!,
    "tom-types-in-x.bin": outgoing(s.tom)!,
    "tom-types-in-y.bin": outgoing(tomInY)!,
  };
}

// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/blockMoves.test.ts`
// les réécrit ; sans la variable, le test les compare, octet pour octet.
test("les octets du test de l'acteur sont ceux qu'écrit y-prosemirror", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", FIXTURES);
  const bytes = await fixtureBytes();
  if (import.meta.env.EXPORT_FIXTURES) {
    await fs.mkdir(dir, { recursive: true });
    for (const [name, data] of Object.entries(bytes)) await fs.writeFile(path.join(dir, name), data);
    return;
  }
  for (const [name, data] of Object.entries(bytes)) {
    const committed = await fs.readFile(path.join(dir, name));
    expect([...data], `${name} (EXPORT_FIXTURES=1 le réécrit)`).toEqual([...committed]);
  }
});
