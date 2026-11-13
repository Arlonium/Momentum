// Deux personnes sur la même page, des gestes au hasard : ce que la forme à clés (R220) et
// `followBlock` promettent ensemble, tenu sur des graines fixées. C'est l'essai de la relecture de
// la Task 13a, borné pour tenir dans la suite.
//
// Deux vrais éditeurs BlockNote 0.57 montés avec `followBlock`, l'extension de l'éditeur de la
// page, et le document du serveur (`test/editors.ts`). À chaque pas, au hasard : quelqu'un fait un
// geste, ou reçoit une part de ce que l'autre a écrit, dans l'ordre où il l'a écrit (le serveur ne
// livre jamais à l'envers, R131). Chacun tape là où est son curseur, sans le reposer avant chaque
// frappe : c'est donc le curseur que `followBlock` a suivi, ou pas, qui dit où va le texte. (Les
// essais au hasard de `keyedBlocks.test.ts` montent leurs éditeurs sans l'extension et tapent à
// une place donnée : ils éprouvent le correctif de y-prosemirror seul.)
//
// Le régime est celui que vise R220. Chacun déplace ses blocs : à la souris, en deux temps (la
// prise, puis le dépôt, entre lesquels il peut recevoir), ou au clavier ; un bloc, ou plusieurs
// voisins d'un coup. Tous tapent, partout, annulent et rétablissent. Et chacun reste chez soi
// (`apart`) : il ne prend que ses blocs, ne les dépose que parmi les siens ou au premier niveau de
// la page, ne les fait pas entrer au clavier sous le bloc d'un autre, et lâche sans déposer ce
// qu'il tient si un autre y a glissé le sien. Hors de cette retenue commencent les limites
// écrites, que cet essai ne tient pas : le dernier test les compte.
//
// Chaque frappe est un jeton unique, qui nomme son auteur et le bloc où était son curseur. Après
// un échange complet, sur une page plate (`FLAT`) comme sur une page à sous-blocs (`NESTED`) :
//  - aucun jeton en double, aucun dans un autre bloc que le sien ;
//  - aucun bloc en double, aucun bloc perdu ;
//  - les trois documents sont égaux, et chaque éditeur montre le sien, à la fin comme après
//    chaque geste et chaque réception ;
//  - aucune exception.
// Un jeton peut manquer : tapé dans un bloc pendant qu'un autre le déplaçait (R222), ou annulé.
//
// L'essai mord. Mesuré le 2026-11-13 sur 100 graines de 100 pas, page plate : sans `followBlock`,
// 231 jetons dans un autre bloc (58 graines) ; avec y-prosemirror d'origine, 395 jetons en double
// et 1 187 dans un autre bloc (99 graines) ; avec la règle d'avant pour un groupe de blocs
// (R227 : les blocs croisés recréés), 21 blocs en double (19 graines).
//
// `MEMENTUM_FUZZ_SEEDS=300` (et `MEMENTUM_FUZZ_STEPS`) élargit l'essai, et écrit ses comptes.
import { NodeSelection, Selection, TextSelection } from "prosemirror-state";
import { afterAll, describe, expect, test } from "vitest";
import { yUndoPluginKey, yXmlFragmentToProseMirrorRootNode } from "y-prosemirror";
import * as Y from "yjs";
import { containerOf, peer, press, REMOTE, seedState, settle, type Peer, type PmNode, type Seed } from "../test/editors";
import { FRAGMENT } from "./collab";
import { followBlock } from "./followBlock";

/** Le nombre de graines par forme de page (et sans retenue), et de pas par graine. */
const SEEDS = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 20);
const LOOSE_SEEDS = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 10);
const STEPS = Number(import.meta.env.MEMENTUM_FUZZ_STEPS ?? 100);

const paragraph = (id: string, children?: Seed) => ({
  id,
  type: "paragraph" as const,
  content: `Bloc ${id.slice(1)}`,
  children,
});

/** Huit paragraphes, b0 à b7. */
const FLAT: Seed = Array.from({ length: 8 }, (_, i) => paragraph(`b${i}`));

/** Neuf paragraphes sur trois niveaux. */
const NESTED: Seed = [
  paragraph("b0", [paragraph("b1"), paragraph("b2", [paragraph("b3")])]),
  paragraph("b4"),
  paragraph("b5", [paragraph("b6")]),
  paragraph("b7"),
  paragraph("b8"),
];

/** Des nombres au hasard, les mêmes à chaque exécution pour une même graine (mulberry32). */
function random(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

// Un jeton : un caractère d'ouverture à lui (zone privée d'Unicode), l'initiale de son auteur, son
// rang, le bloc où était le curseur, et « › ». Sans l'ouverture unique, le diff de texte de
// y-prosemirror peut attribuer à une frappe l'ouverture du jeton voisin, et une annulation le casse.
const TOKEN = /[-]([A-Z])(\d+)@([^›]*)›/g;
const OPEN = /[-]/g;
/** Un texte lisible dans un message : les ouvertures des jetons écrites « ‹ ». */
const readable = (text: string) => text.replace(OPEN, "‹");

/** Le texte d'un nœud de texte de ProseMirror, un contenu en ligne sans texte écrit « ◆ ». */
const textOf = (node: PmNode) => node.textBetween(0, node.content.size, "", "◆");

type Container = { id: string; pos: number; node: PmNode; parent: PmNode };

/** Les blocs du document `doc`, dans l'ordre, chacun avec le groupe qui le porte. */
function containers(doc: PmNode): Container[] {
  const found: Container[] = [];
  doc.descendants((node, pos, parent) => {
    if (node.type.name === "blockContainer" && parent) {
      found.push({ id: String(node.attrs.id), pos, node, parent });
    }
    return true;
  });
  return found;
}

/** Les places d'un texte où poser un curseur : pas au milieu d'un jeton. */
function freeOffsets(text: string): number[] {
  const within = new Set<number>();
  for (const token of text.matchAll(/[-][^›]*›/g)) {
    for (let at = token.index + 1; at < token.index + token[0].length; at += 1) within.add(at);
  }
  return Array.from({ length: text.length + 1 }, (_, at) => at).filter((at) => !within.has(at));
}

/** Les ids des blocs du document Yjs `doc`, dans l'ordre, et pour chacun les jetons de son texte. */
function storedTokens(doc: Y.Doc): { id: string; tokens: string[] }[] {
  const found: { id: string; tokens: string[] }[] = [];
  const walk = (parent: Y.XmlElement | Y.XmlFragment) =>
    parent.toArray().forEach((child) => {
      if (!(child instanceof Y.XmlElement)) return;
      if (child.nodeName === "blockContainer") {
        const content = child.get(0) as Y.XmlElement;
        const text = content
          .toArray()
          .map((piece) =>
            piece instanceof Y.XmlText
              ? piece
                  .toDelta()
                  .map((d: { insert: unknown }) => (typeof d.insert === "string" ? d.insert : "◆"))
                  .join("")
              : "◆",
          )
          .join("");
        found.push({ id: String(child.getAttribute("id")), tokens: [...text.matchAll(TOKEN)].map((m) => m[0]) });
      }
      walk(child);
    });
  walk(doc.getXmlFragment(FRAGMENT));
  return found;
}

/** Une page en une ligne, pour les messages : `{b0:Bloc 0[{b1:Bloc 1}]}{b4:…}`. */
function pageOf(doc: Y.Doc): string {
  const walk = (parent: Y.XmlElement | Y.XmlFragment): string =>
    parent
      .toArray()
      .map((child) => {
        if (!(child instanceof Y.XmlElement)) {
          return (child as Y.XmlText)
            .toDelta()
            .map((d: { insert: unknown }) => (typeof d.insert === "string" ? d.insert : "◆"))
            .join("");
        }
        const inner = walk(child);
        if (child.nodeName === "blockContainer") return `{${child.getAttribute("id")}:${inner}}`;
        return child.nodeName === "blockGroup" ? `[${inner}]` : inner;
      })
      .join("");
  return readable(walk(doc.getXmlFragment(FRAGMENT)));
}

/** Une personne : son éditeur, ce qu'elle a écrit et à qui elle l'a déjà envoyé, ce qu'elle tient. */
type Actor = Peer & {
  /** L'initiale qui signe ses jetons. */
  key: string;
  /** A-t-elle le droit de déplacer le bloc `id` ? */
  owns: (id: string) => boolean;
  /** Ses mises à jour, dans l'ordre où elle les a écrites. */
  outbox: Uint8Array[];
  /** Combien chaque destinataire en a déjà reçu. */
  sent: Map<string, number>;
  /** Tient-elle des blocs au bout de sa souris ? */
  holding: boolean;
};

/** Ce qu'une graine a donné. */
type Outcome = {
  seed: number;
  typed: number;
  moves: number;
  undone: number;
  /** Les jetons présents à la fin, comptés une fois. */
  present: number;
  /** Les jetons présents plus d'une fois. */
  doubled: string[];
  /** Les jetons présents dans un autre bloc que celui où leur auteur tapait. */
  misplaced: string[];
  /** Les ids présents plus d'une fois. */
  twins: string[];
  /** Les blocs de la page de départ absents à la fin. */
  lost: string[];
  /** Un éditeur ne montrait pas son document, après un geste ou une réception. */
  unfaithful: string[];
  /** Un document n'est pas celui du serveur, après l'échange complet. */
  diverged: string[];
  /** Les exceptions, d'un geste ou d'une réception. */
  errors: string[];
  page: string;
  journal: string;
};

/**
 * Joue la graine `seed` sur la page `shape`, en `steps` pas. `apart` : chacun reste chez soi, comme
 * le dit l'en-tête ; sinon les mêmes gestes, sans cette retenue.
 */
async function play(shape: Seed, seed: number, steps: number, { apart = true } = {}): Promise<Outcome> {
  const next = random(seed * 7919 + 13);
  const pick = <T>(list: T[]): T => list[Math.floor(next() * list.length)];
  const base = await seedState(shape);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  const start = storedTokens(server).map((b) => b.id);
  // Chacun a la moitié des blocs ; l'ordre des identifiants de client change avec la graine.
  const clientIds = seed % 2 === 0 ? [2, 3] : [3, 2];
  const actors: Actor[] = [];
  for (const [index, name] of ["Mona", "Tom"].entries()) {
    const p = await peer(name, clientIds[index], base, [followBlock]);
    const mine = new Set(start.filter((_, at) => (at < start.length / 2 ? 0 : 1) === index));
    actors.push({ ...p, key: name[0], owns: (id) => mine.has(id), outbox: [], sent: new Map(), holding: false });
  }
  const out: Outcome = {
    seed,
    typed: 0,
    moves: 0,
    undone: 0,
    present: 0,
    doubled: [],
    misplaced: [],
    twins: [],
    lost: [],
    unfaithful: [],
    diverged: [],
    errors: [],
    page: "",
    journal: "",
  };
  const journal: string[] = [];

  /** L'éditeur de `p` montre-t-il son document ? (Chaque éditeur a son schéma : le sien.) */
  const faithful = (p: Actor, when: string) => {
    const fromY = yXmlFragmentToProseMirrorRootNode(p.doc.getXmlFragment(FRAGMENT), p.editor.pmSchema);
    if (!fromY.eq(p.editor.prosemirrorState.doc)) {
      out.unfaithful.push(`${p.name}, ${when} (pas ${journal.length})`);
    }
  };
  /** Ce que `p` vient d'écrire passe dans ce qu'il a à envoyer. */
  const flush = (p: Actor) => p.outbox.push(...p.writes.splice(0));
  /** `p` fait le geste `gesture`, nommé `label`. */
  const local = (p: Actor, label: string, gesture: () => void) => {
    try {
      gesture();
    } catch (error) {
      out.errors.push(`${p.name} ${label} : ${String(error)}`);
    }
    flush(p);
    journal.push(`${p.name} ${label}`);
    faithful(p, `après « ${label} »`);
  };
  /** La frappe suivante de `p` ne s'annulera pas avec ce qu'il a fait avant. */
  const stopCapturing = (p: Actor) =>
    (
      yUndoPluginKey.getState(p.editor.prosemirrorState) as { undoManager: Y.UndoManager } | undefined
    )?.undoManager.stopCapturing();

  /** Où est le curseur de `p`, s'il est dans un texte : ce qui le précède, et le bloc qui le porte. */
  const cursorOf = (p: Actor): { before: string; home: string } | null => {
    const { selection } = p.editor.prosemirrorState;
    const { $from } = selection;
    if (!(selection instanceof TextSelection) || !selection.empty || !$from.parent.inlineContent) return null;
    for (let depth = $from.depth; depth > 0; depth -= 1) {
      const node = $from.node(depth);
      if (node.type.name === "blockContainer") {
        return { before: $from.parent.textBetween(0, $from.parentOffset, "", "◆"), home: String(node.attrs.id) };
      }
    }
    return null;
  };
  /** `p` pose son curseur dans un bloc au hasard, hors de tout jeton. */
  const place = (p: Actor) => {
    const target = pick(containers(p.editor.prosemirrorState.doc));
    const offset = pick(freeOffsets(textOf(target.node.firstChild!)));
    local(p, `pose son curseur dans ${target.id}`, () =>
      p.editor.transact((tr) => tr.setSelection(TextSelection.create(tr.doc, target.pos + 2 + offset))),
    );
  };
  /** Un bloc de `p`, au hasard, et parfois ceux de ses voisins du dessous qui sont aussi à lui. */
  const own = (p: Actor): Container[] => {
    const blocks = containers(p.editor.prosemirrorState.doc);
    const mine = blocks.filter((b) => p.owns(b.id));
    if (mine.length === 0) return [];
    const first = pick(mine);
    const run = [first];
    const wanted = next() < 0.35 ? 2 + Math.floor(next() * 2) : 1;
    while (run.length < wanted) {
      const last = run[run.length - 1];
      const below = blocks.find((b) => b.parent === first.parent && b.pos === last.pos + last.node.nodeSize);
      if (below === undefined || !p.owns(below.id)) break;
      run.push(below);
    }
    return run;
  };

  /** Le dépôt de ce que tient `p` : le dépôt de ProseMirror, puis le curseur que laisse BlockNote. */
  const drop = (p: Actor) => {
    p.holding = false;
    const view = p.editor.prosemirrorView;
    const { dragging } = view;
    view.dragging = null;
    if (dragging === null) return;
    const held = new Set<string>();
    dragging.slice.content.descendants((node) => {
      if (node.type.name === "blockContainer") held.add(String(node.attrs.id));
      return true;
    });
    const { doc } = p.editor.prosemirrorState;
    const top = doc.firstChild;
    // Chez soi : parmi ses blocs, ou au premier niveau de la page.
    const places = containers(doc).filter(
      (b) => !held.has(b.id) && (!apart || p.owns(b.id) || b.parent === top),
    );
    // Un autre a déposé son bloc entre ceux qu'on tient : le dépôt l'emporterait avec eux.
    const foreign = [...held].some((id) => !p.owns(id));
    if (places.length === 0 || (apart && foreign)) {
      journal.push(`${p.name} lâche ${[...held].join(", ")} sans déposer`);
      return;
    }
    const where = pick(places);
    const under = next() < 0.5;
    out.moves += 1;
    local(p, `dépose ${[...held].join(", ")} ${under ? "sous" : "au-dessus de"} ${where.id}`, () => {
      const { content } = dragging.slice;
      p.editor.transact((tr) => {
        const target = containerOf(tr.doc, where.id);
        const spot = under ? target.pos + target.node.nodeSize : target.pos;
        tr.deleteSelection();
        const at = tr.mapping.map(spot);
        tr.insert(at, content);
        tr.setSelection(
          content.childCount === 1
            ? NodeSelection.create(tr.doc, at)
            : TextSelection.between(tr.doc.resolve(at), tr.doc.resolve(at + content.size)),
        );
      });
      p.editor.transact((tr) => {
        const { selection } = tr;
        if (!(selection instanceof NodeSelection)) tr.setSelection(TextSelection.create(tr.doc, selection.to));
        else if (selection.node.firstChild?.inlineContent) {
          tr.setSelection(TextSelection.create(tr.doc, selection.from + 2 + selection.node.firstChild.content.size));
        }
      });
    });
  };

  /** Un geste de `p`, au hasard. */
  const act = (p: Actor) => {
    if (p.holding) return drop(p);
    const roll = next();
    if (roll < 0.42) {
      const at = cursorOf(p);
      // Pas de frappe au milieu d'un jeton : elle le couperait, et il ne se compterait plus.
      const inToken =
        at !== null && Math.max(-1, ...[...at.before.matchAll(OPEN)].map((m) => m.index)) > at.before.lastIndexOf("›");
      if (at === null || inToken) return place(p);
      const token = `${String.fromCharCode(0xe000 + out.typed)}${p.key}${out.typed}@${at.home}›`;
      if (next() < 0.3) stopCapturing(p);
      out.typed += 1;
      return local(p, `tape ${readable(token)}`, () => p.editor.transact((tr) => tr.insertText(token)));
    }
    stopCapturing(p);
    if (roll < 0.5) return place(p);
    if (roll < 0.68) {
      const run = own(p);
      if (run.length === 0) return;
      // Le début d'un glisser : BlockNote sélectionne en bloc ce qu'on prend, ProseMirror le garde.
      local(p, `prend ${run.map((b) => b.id).join(", ")}`, () => {
        p.editor.transact((tr) => {
          const first = containerOf(tr.doc, run[0].id);
          const last = containerOf(tr.doc, run[run.length - 1].id);
          tr.setSelection(
            Selection.fromJSON(tr.doc, { type: "multiple-node", anchor: first.pos, head: last.pos + last.node.nodeSize }),
          );
        });
        const view = p.editor.prosemirrorView;
        view.dragging = { slice: view.state.selection.content(), move: true };
      });
      p.holding = true;
      return;
    }
    if (roll < 0.8) {
      const run = own(p);
      if (run.length === 0) return;
      const down = next() < 0.5;
      // Au clavier, un bloc entre sous le voisin qu'il croise si ce voisin a des sous-blocs.
      const edge = down ? run[run.length - 1] : run[0];
      const beside = containers(p.editor.prosemirrorState.doc).find(
        (b) =>
          b.parent === edge.parent &&
          (down ? b.pos === edge.pos + edge.node.nodeSize : b.pos + b.node.nodeSize === edge.pos),
      );
      if (apart && beside !== undefined && beside.node.childCount > 1 && !p.owns(beside.id)) return;
      out.moves += 1;
      return local(p, `${down ? "descend" : "monte"} ${run.map((b) => b.id).join(", ")} au clavier`, () => {
        if (run.length === 1) p.editor.setTextCursorPosition(run[0].id, "end");
        else {
          p.editor.transact((tr) => {
            const first = containerOf(tr.doc, run[0].id);
            const last = containerOf(tr.doc, run[run.length - 1].id);
            tr.setSelection(
              TextSelection.create(tr.doc, first.pos + 2, last.pos + 2 + last.node.firstChild!.content.size),
            );
          });
        }
        press(p, down ? "ArrowDown" : "ArrowUp", { ctrlKey: true, shiftKey: true });
      });
    }
    out.undone += 1;
    if (roll < 0.93) return local(p, "annule", () => void p.editor.undo());
    return local(p, "rétablit", () => void p.editor.redo());
  };

  /** `to` (le serveur : `null`) reçoit de `from` ses `count` mises à jour suivantes, dans l'ordre. */
  const deliver = async (from: Actor, to: Actor | null, count: number) => {
    const key = to === null ? "serveur" : to.name;
    const seen = from.sent.get(key) ?? 0;
    const batch = from.outbox.slice(seen, seen + count);
    if (batch.length === 0) return false;
    from.sent.set(key, seen + batch.length);
    if (to === null) {
      for (const update of batch) Y.applyUpdate(server, update, REMOTE);
      return true;
    }
    try {
      for (const update of batch) Y.applyUpdate(to.doc, update, REMOTE);
      await settle();
    } catch (error) {
      out.errors.push(`${to.name} reçoit de ${from.name} : ${String(error)}`);
    }
    // Un éditeur peut écrire en recevant.
    flush(to);
    journal.push(`${to.name} reçoit ${batch.length} de ${from.name}`);
    faithful(to, `après avoir reçu de ${from.name}`);
    return true;
  };

  for (let step = 0; step < steps; step += 1) {
    if (next() < 0.34) {
      const from = pick(actors);
      const to = pick([...actors.filter((p) => p !== from), null]);
      await deliver(from, to, 1 + Math.floor(next() * 3));
    } else {
      act(pick(actors));
    }
  }
  // Qui tient encore des blocs les lâche où ils sont : un glisser abandonné.
  for (const p of actors) {
    p.holding = false;
    p.editor.prosemirrorView.dragging = null;
  }
  // L'échange complet : jusqu'à ce que plus personne n'ait rien à envoyer.
  for (let round = 0; ; round += 1) {
    let sent = false;
    for (const from of actors) {
      for (const to of [...actors.filter((p) => p !== from), null]) {
        if (await deliver(from, to, Number.MAX_SAFE_INTEGER)) sent = true;
      }
    }
    if (!sent) break;
    if (round === 8) {
      out.errors.push("les éditeurs écrivent encore après huit échanges");
      break;
    }
  }

  const xml = (doc: Y.Doc) => doc.getXmlFragment(FRAGMENT).toString();
  for (const p of actors) {
    if (xml(p.doc) !== xml(server)) out.diverged.push(p.name);
    faithful(p, "à la fin");
  }
  const blocks = storedTokens(server);
  const count = <T>(list: T[]) => {
    const seen = new Map<T, number>();
    list.forEach((item) => seen.set(item, (seen.get(item) ?? 0) + 1));
    return seen;
  };
  out.twins = [...count(blocks.map((b) => b.id))].filter(([, n]) => n > 1).map(([id]) => id);
  out.lost = start.filter((id) => !blocks.some((b) => b.id === id));
  const tokens = blocks.flatMap((b) => b.tokens.map((token) => ({ token, in: b.id })));
  const times = count(tokens.map((t) => t.token));
  out.present = times.size;
  out.doubled = [...times].filter(([, n]) => n > 1).map(([token]) => readable(token));
  out.misplaced = tokens
    .filter((t) => t.in !== t.token.slice(t.token.indexOf("@") + 1, -1))
    .map((t) => `${readable(t.token)} dans ${t.in}`);
  out.page = pageOf(server);
  out.journal = journal.join(" ; ");
  return out;
}

const seeds = (count: number) => Array.from({ length: count }, (_, i) => i + 1);

/** Les formes de page, et ce que chaque graine y a donné. */
const SHAPES = [
  { name: "plate", shape: FLAT, outcomes: [] as Outcome[] },
  { name: "à sous-blocs", shape: NESTED, outcomes: [] as Outcome[] },
];
/** La page à sous-blocs, sans la retenue de l'en-tête. */
const loose: Outcome[] = [];

describe.each(SHAPES)("moves_and_typing_at_random_keep_the_promise_of_keyed_blocks (page $name)", (page) => {
  test.each(seeds(SEEDS))("graine %i", async (seed) => {
    const out = await play(page.shape, seed, STEPS);
    page.outcomes.push(out);
    const told = `graine ${seed} : ${out.journal}\npage : ${out.page}`;
    expect(out.errors, told).toEqual([]);
    expect(out.unfaithful, told).toEqual([]);
    expect(out.diverged, told).toEqual([]);
    expect(out.twins, told).toEqual([]);
    expect(out.lost, told).toEqual([]);
    expect(out.doubled, told).toEqual([]);
    expect(out.misplaced, told).toEqual([]);
  });

  test("l'essai fait ce qu'il dit : des frappes dont beaucoup restent, des déplacements, des annulations", () => {
    const sum = (f: (out: Outcome) => number) => page.outcomes.reduce((n, out) => n + f(out), 0);
    expect(page.outcomes).toHaveLength(SEEDS);
    expect(sum((out) => out.typed)).toBeGreaterThan(15 * SEEDS);
    expect(sum((out) => out.present)).toBeGreaterThan(10 * SEEDS);
    expect(sum((out) => out.moves)).toBeGreaterThan(10 * SEEDS);
    expect(sum((out) => out.undone)).toBeGreaterThan(5 * SEEDS);
  });
});

// Les mêmes gestes sans la retenue de l'en-tête : chacun ne prend toujours que ses blocs, mais
// les dépose n'importe où, y compris sous le bloc d'un autre, les fait entrer au clavier sous le
// voisin d'un autre, et dépose ce qu'il tient même si un autre y a glissé le sien. C'est la
// limite écrite : un bloc déplacé est recréé avec les sous-blocs que son auteur lui connaissait,
// donc sans celui qu'un autre vient d'y mettre ; et le bloc d'un autre, pris dans un déplacement,
// peut être déplacé deux fois à la fois, ce qui le laisse en double, sous des ids neufs.
//
// Ce qui tient encore : aucune exception, et les trois documents égaux. Ce qui ne tient plus,
// mesuré le 2026-11-13 sur 300 graines de 100 pas (6 805 jetons tapés, 3 958 présents à la fin) :
// 34 jetons en double (13 graines), 96 dans un autre bloc (26), 8 blocs en double à la fin (8),
// 543 blocs dont l'id de départ a disparu, perdus ou en double puis renommés (206 graines), et 129
// fois un éditeur qui ne montre pas son document, le temps que BlockNote renomme un bloc en
// double (38). Chez soi, les mêmes 300 graines donnent zéro partout, sur la page plate (6 722
// jetons tapés, 4 731 présents, 5 031 déplacements, 3 398 annulations ou rétablissements) comme
// sur la page à sous-blocs (6 816 jetons, 4 828 présents).
describe("without_staying_apart_the_documents_still_converge (page à sous-blocs)", () => {
  test.each(seeds(LOOSE_SEEDS))("graine %i", async (seed) => {
    const out = await play(NESTED, seed, STEPS, { apart: false });
    loose.push(out);
    const told = `graine ${seed} : ${out.journal}\npage : ${out.page}`;
    expect(out.errors, told).toEqual([]);
    expect(out.diverged, told).toEqual([]);
  });
});

afterAll(() => {
  if (!import.meta.env.MEMENTUM_FUZZ_SEEDS) return;
  const all = [...SHAPES.map((page) => [page.name, page.outcomes] as const), ["à sous-blocs, sans retenue", loose] as const];
  for (const [name, list] of all) {
    const sum = (f: (out: Outcome) => number) => list.reduce((n, out) => n + f(out), 0);
    const where = (f: (out: Outcome) => number) => list.filter((out) => f(out) > 0).length;
    console.log(
      `page ${name}, ${list.length} graines de ${STEPS} pas : ${sum((o) => o.typed)} jetons tapés, ` +
        `${sum((o) => o.present)} présents, ${sum((o) => o.moves)} déplacements, ` +
        `${sum((o) => o.undone)} annulations ou rétablissements ; ` +
        `jetons en double ${sum((o) => o.doubled.length)} (${where((o) => o.doubled.length)} graines), ` +
        `dans un autre bloc ${sum((o) => o.misplaced.length)} (${where((o) => o.misplaced.length)}), ` +
        `blocs en double ${sum((o) => o.twins.length)} (${where((o) => o.twins.length)}), ` +
        `blocs perdus ${sum((o) => o.lost.length)} (${where((o) => o.lost.length)}), ` +
        `éditeur ≠ document ${sum((o) => o.unfaithful.length)} (${where((o) => o.unfaithful.length)}), ` +
        `documents ≠ ${sum((o) => o.diverged.length)} (${where((o) => o.diverged.length)}), ` +
        `exceptions ${sum((o) => o.errors.length)} (${where((o) => o.errors.length)})`,
    );
  }
});
