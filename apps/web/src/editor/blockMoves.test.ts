// Ce que chaque déplacement de bloc écrit dans le document partagé, et ce que devient une frappe
// faite en même temps par quelqu'un d'autre (limite R39 ; Task 12, puis Task 13).
//
// Deux vrais éditeurs BlockNote 0.57, ceux de Mona et de Tom, liés à deux `Y.Doc` aux
// identifiants de client fixes (`test/editors.ts`). Chacun fait son geste depuis le même état ;
// leurs mises à jour s'échangent ensuite à la main, et passent par un troisième document, celui
// du « serveur », dans un ordre donné. L'issue d'un conflit dépend de l'ordre des identifiants
// de client : chaque scénario est joué dans les deux (`ORDERS`), et dans les deux ordres d'envoi.
//
// Depuis la forme à clés (R220, `keyedBlocks.test.ts`), un bloc déplacé au même niveau est
// supprimé et recréé avec le même id, et les autres ne sont pas touchés : ce qui est tapé dans un
// bloc que personne ne déplace reste à sa place, et deux déplacements de blocs différents sont
// justes. Reste ce qui est tapé dans le bloc déplacé lui-même, que les clients seuls perdent.
//
// Chaque scénario (`SCENARIOS`, S1 à S11 : les noms du registre du lot 3, « Task 12 ») attend le
// comportement voulu : les blocs dans l'ordre, chaque id une seule fois, chaque texte dans son
// bloc, types et enfants compris, rien en double, Mona, Tom et le serveur d'accord. Ceux qui
// échouent encore portent leur raison (`loss`, par ordre d'envoi) et sont marqués `test.fails` :
// la suite reste verte, et le jour où l'un passe, elle le dit. `MEMENTUM_SHOW_R39=1` les lance
// comme des tests ordinaires, pour voir leur échec. Seule l'exigence du scénario peut être cet
// échec : une précondition manquée (un geste qui n'a pas fait chez son auteur ce que le scénario
// dit) ou une exception d'un éditeur lève, et ne passe jamais pour la perte attendue
// (`outcomeOf`). Chaque geste a son témoin, vert : seul, il fait ce qu'on attend. Une dernière
// partie relit ce que chaque geste, fait seul, écrit dans le document : les éléments Yjs gardés
// (et l'id qu'ils portent ensuite), supprimés, créés.
//
// Il n'y a pas de serveur ici : ces tests disent ce que les clients écrivent et ce que donne leur
// fusion. Ce que le serveur en fait (reporter sur le bloc recréé ce que son auteur n'avait pas
// vu, R221 ; ne garder qu'un exemplaire d'un bloc déplacé deux fois) se vérifie dans
// `crates/mementum-sync/tests/moves.rs`, avec des octets que cette suite produit (fin du
// fichier), et dans un navigateur, dans `e2e/block-move.spec.ts`.
import { describe, expect, test } from "vitest";
import * as Y from "yjs";
import {
  A,
  block,
  containerOf,
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
  typeAt,
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
  /**
   * Pourquoi l'exigence n'est pas encore tenue, selon qui envoie le premier (un ordre d'envoi
   * absent passe). Absent : le scénario passe dans les deux.
   */
  loss?: Partial<Record<First, string>>;
};

/**
 * Tom a tapé dans le bloc que Mona déplace, et les clients seuls perdent sa frappe (elle est dans
 * l'élément que le déplacement supprime). Ce qu'il en advient dépend de l'ordre d'arrivée au
 * serveur. La frappe d'abord : le serveur l'a quand le déplacement arrive, et la reportera sur le
 * bloc recréé (R221, Task 13b). Le déplacement d'abord : la frappe, faite sans l'avoir vu, arrive
 * après lui, et reste perdue (R222).
 */
const TYPED_IN_THE_MOVED_BLOCK = {
  tom: "attend le report du serveur, Task 13b",
  mona: "limite R222 : la frappe arrive après le déplacement",
};

/**
 * Le même bloc déplacé par les deux : chacun le recrée à sa place, et il est deux fois dans la
 * page, au même id. Le serveur n'en garde qu'un (son dédoublonnage voit enfin le doublon : la
 * mise à jour insère des éléments ; test de l'acteur à la Task 13b).
 */
const MOVED_TWICE = {
  mona: "attend le dédoublonnage du serveur, Task 13b",
  tom: "attend le dédoublonnage du serveur, Task 13b",
};

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
    loss: TYPED_IN_THE_MOVED_BLOCK,
  },
  {
    key: "s2",
    title: "Y est un titre ; Tom tape dans X pendant que Mona glisse X sous Z",
    seed: TITLED,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [TITLE_Y, Z, X], tom: [typedIn.x, TITLE_Y, Z] },
    wanted: [TITLE_Y, Z, typedIn.x],
    loss: TYPED_IN_THE_MOVED_BLOCK,
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
    // L'enfant est recréé avec son parent.
    loss: TYPED_IN_THE_MOVED_BLOCK,
  },
  {
    key: "s4",
    title: "Tom tape dans Y, que personne ne déplace, pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => typeAtEnd(p, "y", TYPED),
    local: { mona: [Y_, Z, X], tom: [X, typedIn.y, Z] },
    wanted: [typedIn.y, Z, X],
  },
  {
    key: "s5",
    title: "Tom tape dans Z pendant que Mona glisse W tout en haut",
    seed: XYZW,
    mona: (p) => drag(p, "w", { above: "x" }),
    tom: (p) => typeAtEnd(p, "z", TYPED),
    local: { mona: [W, X, Y_, Z], tom: [X, Y_, typedIn.z, W] },
    wanted: [W, X, Y_, typedIn.z],
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
    loss: MOVED_TWICE,
  },
  {
    key: "s7",
    title: "Tom tape dans X pendant que Mona le descend d'une place au clavier",
    seed: XYZ,
    mona: (p) => keyboardDown(p, "x"),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [Y_, X, Z], tom: [typedIn.x, Y_, Z] },
    wanted: [Y_, typedIn.x, Z],
    loss: TYPED_IN_THE_MOVED_BLOCK,
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
    loss: TYPED_IN_THE_MOVED_BLOCK,
  },
  {
    key: "s9",
    title: "Tom tape dans X pendant que Mona en fait l'enfant de W (Tab)",
    seed: W_FIRST,
    mona: (p) => tab(p, "x"),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [wWithChild("Bloc X"), Y_], tom: [W, typedIn.x, Y_] },
    wanted: [wWithChild(`Bloc X${TYPED}`), Y_],
    // Tab écrivait déjà une copie et une suppression : la forme à clés n'y change rien.
    loss: TYPED_IN_THE_MOVED_BLOCK,
  },
  {
    key: "s10",
    title: "Tom tape dans X pendant que Mona le remonte d'un niveau (Maj+Tab)",
    seed: W_PARENT,
    mona: (p) => shiftTab(p, "x"),
    tom: (p) => typeAtEnd(p, "x", TYPED),
    local: { mona: [W, X, Y_], tom: [wWithChild(`Bloc X${TYPED}`), Y_] },
    wanted: [W, typedIn.x, Y_],
    loss: TYPED_IN_THE_MOVED_BLOCK,
  },
  {
    key: "s11",
    title: "sans aucune frappe, Mona glisse X sous Z, Tom glisse Y sous A",
    seed: XYZA,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => drag(p, "y", { under: "a" }),
    local: { mona: [Y_, Z, X, A], tom: [X, Z, A, Y_] },
    wanted: [Z, X, A, Y_],
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

const FIRSTS = ["mona", "tom"] as const;
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
        for (const first of FIRSTS) {
          const name = `${sc.key.toUpperCase()} : ${sc.title} ; ${FIRST_LABEL[first]}`;
          const body = async () => expectEverywhere(await play(sc, order, first), sc.wanted);
          const reason = sc.loss?.[first];
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

// Ce qu'écrivent y-prosemirror 1.3.7, corrigé (`patches/y-prosemirror@1.3.7.patch`, R220), et
// BlockNote 0.57 pour chaque geste, fait seul : ce sur quoi reposent le tableau du registre et le
// report du serveur (Task 13b). Avant le correctif, un bloc déplacé au même niveau n'était ni
// supprimé ni créé : les éléments de la plage changeaient d'id sur place (x→y, y→z, z→x), et leur
// texte était récrit. Une mise à jour de l'une ou de l'autre dépendance peut changer ces formes ;
// ces tests le diront.
describe("ce que chaque geste écrit dans le document", () => {
  const order = ORDERS[0];

  test("glisser au même niveau : le bloc glissé est supprimé et recréé, aucun autre n'est touché", async () => {
    const s = await scene(XYZ, order);
    expect(written(s.mona, () => drag(s.mona, "x", { under: "z" }))).toEqual({
      deleted: ["blockContainer x", "paragraph de x"],
      created: ["blockContainer x", "paragraph de x"],
      renamed: {},
    });
  });

  test("glisser de plusieurs places vers le haut : le bloc glissé seul est recréé", async () => {
    const s = await scene(XYZW, order);
    expect(written(s.mona, () => drag(s.mona, "w", { above: "x" }))).toEqual({
      deleted: ["blockContainer w", "paragraph de w"],
      created: ["blockContainer w", "paragraph de w"],
      renamed: {},
    });
  });

  test("déplacer au clavier : le bloc déplacé est recréé, pas celui qu'il croise", async () => {
    const s = await scene(XYZ, order);
    expect(written(s.mona, () => keyboardDown(s.mona, "x"))).toEqual({
      deleted: ["blockContainer x", "paragraph de x"],
      created: ["blockContainer x", "paragraph de x"],
      renamed: {},
    });
  });

  test("glisser par-dessus un autre type : le titre croisé n'est plus touché", async () => {
    const s = await scene(TITLED, order);
    expect(written(s.mona, () => drag(s.mona, "x", { under: "z" }))).toEqual({
      deleted: ["blockContainer x", "paragraph de x"],
      created: ["blockContainer x", "paragraph de x"],
      renamed: {},
    });
  });

  test("glisser un bloc qui a des enfants : le bloc est recréé avec ses enfants", async () => {
    const s = await scene(PARENTED, order);
    const subtree = [
      "blockContainer x",
      "paragraph de x",
      "blockGroup de x",
      "blockContainer x1",
      "paragraph de x1",
    ];
    expect(written(s.mona, () => drag(s.mona, "x", { under: "z" }))).toEqual({
      deleted: subtree,
      created: subtree,
      renamed: {},
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
 * Ce que rejoue le test de l'acteur depuis le lot 3, à la racine du dossier, écrit par
 * y-prosemirror depuis la même page de départ (« Bloc X, Bloc Y, Bloc Z », identifiant de
 * client 1) : le glisser de X sous Z fait par Mona (identifiant 2), et la frappe de Tom
 * (identifiant 3) à la fin de X, ou de Y. Ce sont les octets des scénarios S1 et S4, Mona avant
 * Tom, sous les noms que `moves.rs` inclut.
 */
async function legacyBytes(): Promise<Record<string, Uint8Array>> {
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

/**
 * Des scènes de plus pour le report du serveur (Task 13b, R221) : seuls leurs octets servent, et
 * ce que le serveur doit en faire se décide là-bas. `wanted` : la page qu'on voudrait, quand elle
 * se dit sans choisir à sa place ; `note` : ce que le plan de la Task 13b en dit.
 */
type Extra = {
  key: string;
  title: string;
  seed: Seed;
  mona: (p: Peer) => void;
  tom: (p: Peer) => void;
  wanted?: Shown[];
  note: string;
};

const EXTRAS: Extra[] = [
  {
    key: "start-middle-end",
    title: "Tom tape au début, au milieu et à la fin de X pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => {
      typeAt(p, "x", 0, "Début ");
      typeAt(p, "x", 8, "[milieu]");
      typeAtEnd(p, "x", TYPED);
    },
    wanted: [Y_, Z, block("x", `Début Bl[milieu]oc X${TYPED}`)],
    note: "text_typed_before_the_move_arrives_is_carried : trois frappes, trois places.",
  },
  {
    key: "bold",
    title: "Tom tape en gras à la fin de X pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) =>
      p.editor.transact((tr) => {
        const { pos, node } = containerOf(tr.doc, "x");
        const { pmSchema } = p.editor;
        const bold = pmSchema.text(TYPED, [pmSchema.marks.bold.create()]);
        tr.insert(pos + 2 + node.firstChild!.content.size, bold);
      }),
    wanted: [Y_, Z, typedIn.x],
    note: "carried_text_keeps_its_marks : « de Tom » est en gras (marque `bold`).",
  },
  {
    key: "child-added",
    title: "Tom ajoute un sous-bloc sous X pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) =>
      void p.editor.updateBlock("x", {
        children: [{ id: "x1", type: "paragraph", content: "Enfant de Tom" }],
      }),
    wanted: [Y_, Z, block("x", "Bloc X", { children: [block("x1", "Enfant de Tom")] })],
    note: "child_block_added_under_the_original_follows_it.",
  },
  {
    key: "mover-typed-too",
    title: "Mona tape dans X puis le glisse sous Z, sans que le serveur ait rien vu ; Tom tape dans X",
    seed: XYZ,
    mona: (p) => {
      typeAtEnd(p, "x", " de Mona");
      drag(p, "x", { under: "z" });
    },
    tom: (p) => typeAt(p, "x", 0, "Tom : "),
    note:
      "mover_own_offline_edits_are_not_doubled : la copie de Mona n'est pas le texte que le " +
      "serveur avait ; le plan demande de s'abstenir (« Bloc X de Mona », sans doublon).",
  },
  {
    key: "several-texts",
    title: "X porte une mention (plusieurs textes en ligne) ; Tom tape à sa fin pendant que Mona le glisse sous Z",
    seed: [
      {
        id: "x",
        type: "paragraph",
        content: [
          { type: "text", text: "Voir ", styles: {} },
          { type: "mention", props: { kind: "page", id: "p1", label: "Plan" } },
          { type: "text", text: " ici", styles: {} },
        ],
      },
      { id: "y", type: "paragraph", content: "Bloc Y" },
      { id: "z", type: "paragraph", content: "Bloc Z" },
    ],
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) =>
      p.editor.transact((tr) => {
        const { pos, node } = containerOf(tr.doc, "x");
        tr.insertText(TYPED, pos + 2 + node.firstChild!.content.size);
      }),
    note: "inline_content_with_several_texts_is_left_alone : le plan demande de s'abstenir.",
  },
  {
    key: "concurrent-delete",
    title: "Tom efface « oc » dans X et tape à sa fin pendant que Mona glisse X sous Z",
    seed: XYZ,
    mona: (p) => drag(p, "x", { under: "z" }),
    tom: (p) => {
      p.editor.transact((tr) => {
        const { pos } = containerOf(tr.doc, "x");
        tr.delete(pos + 2 + 2, pos + 2 + 4);
      });
      typeAtEnd(p, "x", TYPED);
    },
    note:
      "concurrent_delete_is_not_carried_and_removes_nothing_typed : la suppression de Tom n'est " +
      "pas reportée (« Bloc X de Tom »), et rien de ce qu'il a tapé n'est retiré.",
  },
];

/** Les blocs d'un état Yjs, lus sans éditeur. */
function pageOf(...updates: Uint8Array[]): Shown[] {
  const doc = new Y.Doc();
  for (const update of updates) Y.applyUpdate(doc, update);
  return stored(doc);
}

/**
 * Les fichiers d'une scène, dans l'ordre d'identifiants `order` : la page de départ, ce qu'écrit
 * Mona, ce qu'écrit Tom (chacun depuis la page de départ, sans avoir rien vu de l'autre), et sa
 * description, pour qui écrit le test de l'acteur.
 */
async function sceneFiles(
  sc: Scenario | Extra,
  order: Order,
): Promise<Record<string, Uint8Array | string>> {
  const s = await scene(sc.seed, order);
  expect(s.quiet, "rien d'écrit à l'ouverture").toBe(true);
  sc.mona(s.mona);
  sc.tom(s.tom);
  if ("local" in sc) requireLocallyDone(s, sc.local);
  const mona = outgoing(s.mona)!;
  const tom = outgoing(s.tom)!;
  const description = {
    scenario: sc.key,
    title: sc.title,
    clients: { start: 1, mona: order.mona, tom: order.tom },
    // Chaque page : ses blocs, chacun avec son id, son type, son texte et ses enfants.
    start: pageOf(s.base),
    monaAlone: pageOf(s.base, mona),
    tomAlone: pageOf(s.base, tom),
    ...("local" in sc
      ? {
          wanted: Array.isArray(sc.wanted[0]) ? sc.wanted : [sc.wanted],
          // Ce que donnent les clients seuls, selon qui arrive le premier au serveur.
          clientsAlone: {
            monaFirst: sc.loss?.mona ?? "juste",
            tomFirst: sc.loss?.tom ?? "juste",
          },
        }
      : { ...(sc.wanted ? { wanted: [sc.wanted] } : {}), note: sc.note }),
  };
  return {
    "base.bin": s.base,
    "mona.bin": mona,
    "tom.bin": tom,
    "scenario.json": `${JSON.stringify(description, null, 2)}\n`,
  };
}

/** Compare `files` à ceux du dossier `dir`, ou les y écrit avec `EXPORT_FIXTURES=1`. */
async function checkOrExport(dir: string, files: Record<string, Uint8Array | string>): Promise<void> {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  if (import.meta.env.EXPORT_FIXTURES) {
    await fs.mkdir(dir, { recursive: true });
    for (const [name, data] of Object.entries(files)) await fs.writeFile(path.join(dir, name), data);
    return;
  }
  for (const [name, data] of Object.entries(files)) {
    const told = `${path.join(dir, name)} (EXPORT_FIXTURES=1 le réécrit)`;
    if (typeof data === "string") {
      expect(data, told).toBe(await fs.readFile(path.join(dir, name), "utf8"));
    } else {
      expect([...data], told).toEqual([...(await fs.readFile(path.join(dir, name)))]);
    }
  }
}

/** Le dossier des octets, dans le dépôt. */
async function fixturesDir(): Promise<string> {
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  return path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", FIXTURES);
}

// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/blockMoves.test.ts`
// les réécrit ; sans la variable, les tests les comparent, octet pour octet.
describe("les octets du test de l'acteur sont ceux qu'écrit le client", () => {
  test("les quatre fichiers que `moves.rs` inclut", async () => {
    await checkOrExport(await fixturesDir(), await legacyBytes());
  });

  // Un dossier par ordre d'identifiants (`mona-first`, `tom-first`), puis un par scène : `s1` à
  // `s11`, et les scènes de plus de la Task 13b.
  for (const order of ORDERS) {
    for (const sc of [...SCENARIOS, ...EXTRAS]) {
      test(`${order.dir}/${sc.key} : ${sc.title}`, async () => {
        const path = await nodeModule<Path>("node:path");
        const files = await sceneFiles(sc, order);
        await checkOrExport(path.join(await fixturesDir(), order.dir, sc.key), files);
      });
    }
  }

  test("les scènes de plus font chez chacun ce que leur titre dit", async () => {
    const alone = async (key: string, who: "mona" | "tom") => {
      const sc = EXTRAS.find((extra) => extra.key === key)!;
      const s = await scene(sc.seed, ORDERS[0]);
      sc[who](s[who]);
      return line(shown(s[who]));
    };
    expect(await alone("start-middle-end", "tom")).toBe(
      "x « Début Bl[milieu]oc X de Tom » | y « Bloc Y » | z « Bloc Z »",
    );
    expect(await alone("bold", "tom")).toBe("x « Bloc X de Tom » | y « Bloc Y » | z « Bloc Z »");
    expect(await alone("child-added", "tom")).toBe(
      "x « Bloc X » [x1 « Enfant de Tom »] | y « Bloc Y » | z « Bloc Z »",
    );
    expect(await alone("mover-typed-too", "mona")).toBe(
      "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Mona »",
    );
    expect(await alone("mover-typed-too", "tom")).toBe(
      "x « Tom : Bloc X » | y « Bloc Y » | z « Bloc Z »",
    );
    expect(await alone("several-texts", "tom")).toBe(
      "x « Voir  ici de Tom » | y « Bloc Y » | z « Bloc Z »",
    );
    expect(await alone("concurrent-delete", "tom")).toBe(
      "x « Bl X de Tom » | y « Bloc Y » | z « Bloc Z »",
    );
  });
});

// ---------------------------------------------------------------------------
// L'état final, rouvert
// ---------------------------------------------------------------------------

// Ce que laisse chaque scénario, rouvert par deux éditeurs neufs : ils montrent la même page, et
// n'écrivent rien en l'ouvrant. Un état qu'un éditeur récrirait à l'ouverture ferait écrire tous
// les onglets à la fois, chacun à sa façon.
describe.each(ORDERS)("l'état final de chaque scénario, rouvert ($label)", (order) => {
  test.each(SCENARIOS)("$key : deux éditeurs le rouvrent sans rien écrire", async (sc) => {
    const s = await play(sc, order, "mona");
    const final = Y.encodeStateAsUpdate(s.server);
    const first = await peer("Rouvre 1", 7, final);
    const second = await peer("Rouvre 2", 8, final);
    expect(first.writes.length + second.writes.length, "rien d'écrit à l'ouverture").toBe(0);
    expect(xml(first.doc)).toBe(xml(s.server));
    expect(xml(second.doc)).toBe(xml(s.server));
    const page = stored(s.server);
    for (const p of [first, second]) {
      // Un bloc en double reçoit de chaque éditeur un id à lui, sans qu'il l'écrive (S6) : les
      // textes sont alors ceux du serveur, dans l'ordre ; sinon, c'est sa page, ids compris.
      if (repeatedIds(page).length === 0) expect(shown(p)).toEqual(page);
      else expect(shown(p).map((b) => b.text)).toEqual(page.map((b) => b.text));
    }
  });
});
