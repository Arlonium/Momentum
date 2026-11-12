// La garde du navigateur contre un état de présence mal formé : la même règle que le serveur
// (`known_shape`, `crates/mementum-sync/src/presence.rs`, dont ces cas reprennent les tests),
// et l'awareness que reçoit l'éditeur, qui ne montre que les états en règle.
import { afterEach, describe, expect, test } from "vitest";
import { applyAwarenessUpdate, Awareness, encodeAwarenessUpdate } from "y-protocols/awareness";
import * as Y from "yjs";
import { guardedAwareness, knownShape } from "./presenceGuard";

/** Une position relative de Yjs, à la fin du fragment de l'éditeur. */
const END = { tname: "document-store", assoc: 0 };
/** Une position dans un bloc : ce que le client écrit quand le curseur est dans du texte. */
const IN_TEXT = {
  type: { client: 1440742776, clock: 6 },
  item: { client: 206716952, clock: 41 },
  assoc: -1,
};
const MONA = { name: "Mona", color: "#B4462B" };

function cursor(anchor: unknown, head: unknown) {
  return { user: MONA, cursor: { anchor, head } };
}

describe("knownShape", () => {
  test("the_shapes_the_client_writes_are_known", () => {
    const known: unknown[] = [
      // L'état initial de y-protocols, puis ce que BlockNote y pose.
      {},
      { user: MONA },
      { user: { name: "Mona", color: "#b4462b" }, cursor: null },
      cursor(END, END),
      cursor(IN_TEXT, END),
      // Ce que le client tolère aussi : il a des valeurs par défaut pour le nom et la
      // couleur, et lit `null` comme une absence.
      { user: {} },
      { user: { name: "Mona" } },
      { user: { color: "#000000" } },
      { cursor: null },
      cursor({ type: null, tname: "document-store", item: null }, END),
      cursor({ item: { client: 0, clock: 0 } }, END),
      cursor(
        { type: { client: Number.MAX_SAFE_INTEGER, clock: 0xffff_ffff }, assoc: -(2 ** 31) },
        END,
      ),
      { user: { name: "é".repeat(100) } },
      // Cent caractères, pas cent unités UTF-16.
      { user: { name: "🚀".repeat(100) } },
    ];
    for (const state of known) {
      expect(knownShape(state), JSON.stringify(state)).toBe(true);
    }
  });

  test("other_shapes_are_not", () => {
    const name = (value: unknown) => ({ user: { name: value } });
    const colour = (value: unknown) => ({ user: { name: "Mona", color: value } });
    const cases: [string, unknown][] = [
      ["pas un objet", []],
      ["pas un objet", "user"],
      ["pas un objet", 7],
      ["pas un objet", null],
      ["pas un objet", undefined],
      ["clé inconnue", { surprise: true }],
      ["clé inconnue à côté d'un état en règle", { user: { name: "Mona" }, x: 1 }],
      ["user nul", { user: null }],
      ["user qui n'est pas un objet", { user: "Mona" }],
      ["user qui n'est pas un objet", { user: 7 }],
      ["user qui est un tableau", { user: [] }],
      ["clé inconnue dans user", { user: { name: "Mona", avatar: "x" } }],
      ["nom vide", name("")],
      ["nom trop long", name("a".repeat(101))],
      ["nom qui n'est pas une chaîne", name(7)],
      ["nom nul", name(null)],
      ["nom avec un caractère de contrôle", name("Mo\u0000na")],
      ["nom avec un saut de ligne", name("Mo\nna")],
      ["nom avec un caractère de contrôle C1", name("Mo\u0085na")],
      [
        "couleur piégée",
        colour(
          "red;position:fixed;inset:0;z-index:99999;background:url(https://evil.example/x.png)",
        ),
      ],
      ["couleur nommée", colour("red")],
      ["couleur à trois chiffres", colour("#f00")],
      ["couleur à huit chiffres", colour("#ff000000")],
      ["couleur avec une suite", colour("#ff0000;")],
      ["couleur avec un saut de ligne", colour("#ff0000\n")],
      ["couleur hors hexadécimal", colour("#ff00zz")],
      ["couleur sans dièse", colour("ff0000f")],
      ["couleur qui n'est pas une chaîne", colour(42)],
      ["couleur nulle", colour(null)],
      ["curseur qui n'est pas un objet", { cursor: 7 }],
      ["curseur vide", { cursor: {} }],
      ["curseur sans head", { cursor: { anchor: END } }],
      ["curseur sans anchor", { cursor: { head: END } }],
      ["clé inconnue dans le curseur", { cursor: { anchor: END, head: END, x: 1 } }],
      ["anchor qui n'est pas un objet", cursor(7, 7)],
      ["head qui n'est pas un objet", cursor(END, 7)],
      ["anchor nul", cursor(null, END)],
      ["position sans rien", cursor({}, END)],
      ["position aux trois champs nuls", cursor({ type: null, tname: null, item: null }, END)],
      ["position avec assoc seul", cursor({ assoc: 0 }, END)],
      ["clé inconnue dans une position", cursor({ tname: "document-store", x: 1 }, END)],
      ["tname d'un autre fragment", cursor({ tname: "autre" }, END)],
      ["tname qui n'est pas une chaîne", cursor({ tname: 7 }, END)],
      ["type qui n'est pas un objet", cursor({ type: 7 }, END)],
      ["type sans clock", cursor({ type: { client: 1 } }, END)],
      [
        "clé inconnue dans un identifiant",
        cursor({ type: { client: 1, clock: 1, x: 1 } }, END),
      ],
      ["client négatif", cursor({ type: { client: -1, clock: 1 } }, END)],
      ["client à virgule", cursor({ type: { client: 1.5, clock: 1 } }, END)],
      ["client en chaîne", cursor({ type: { client: "1", clock: 1 } }, END)],
      [
        "client hors de 53 bits",
        cursor({ type: { client: Number.MAX_SAFE_INTEGER + 1, clock: 0 } }, END),
      ],
      ["clock hors de 32 bits", cursor({ type: { client: 1, clock: 2 ** 32 } }, END)],
      ["item qui n'est pas un objet", cursor({ item: [1, 2] }, END)],
      ["assoc hors de 32 bits", cursor({ tname: "document-store", assoc: 2 ** 31 }, END)],
      ["assoc à virgule", cursor({ tname: "document-store", assoc: 0.5 }, END)],
      ["assoc nul", cursor({ tname: "document-store", assoc: null }, END)],
      ["assoc en chaîne", cursor({ tname: "document-store", assoc: "0" }, END)],
    ];
    for (const [what, state] of cases) {
      expect(knownShape(state), `${what} : ${JSON.stringify(state)}`).toBe(false);
    }
  });

  test("a_key_named___proto___is_an_unknown_key", () => {
    // `JSON.parse` en fait une clé ordinaire, que la règle refuse comme toute autre.
    const state: unknown = JSON.parse('{"user":{"name":"Mona"},"__proto__":{"x":1}}');
    expect(knownShape(state)).toBe(false);
    const user: unknown = JSON.parse('{"user":{"name":"Mona","__proto__":{"color":"red;x"}}}');
    expect(knownShape(user)).toBe(false);
  });
});

describe("guardedAwareness", () => {
  const made: Awareness[] = [];
  afterEach(() => {
    for (const awareness of made.splice(0)) awareness.destroy();
  });

  /** L'awareness d'un onglet. */
  function tab(): Awareness {
    const awareness = new Awareness(new Y.Doc());
    made.push(awareness);
    return awareness;
  }

  /** Fait recevoir à `awareness` l'état `state` d'un autre client, et rend son identifiant. */
  function receive(awareness: Awareness, state: unknown): number {
    const other = tab();
    other.setLocalState(state as Record<string, unknown>);
    applyAwarenessUpdate(awareness, encodeAwarenessUpdate(other, [other.clientID]), "serveur");
    return other.clientID;
  }

  test("a_malformed_remote_state_is_not_shown", () => {
    const real = tab();
    const guarded = guardedAwareness(real);
    const trapped = receive(real, {
      user: { name: "Olga", color: "red;position:fixed;inset:0" },
      cursor: { anchor: END, head: END },
    });
    const broken = receive(real, { user: MONA, cursor: { anchor: 7, head: 7 } });
    const fine = receive(real, cursor(IN_TEXT, END));

    // L'awareness de y-websocket les a tous reçus.
    expect(real.getStates().has(trapped)).toBe(true);
    expect(real.getStates().has(broken)).toBe(true);
    // L'éditeur ne voit que celui qui est en règle.
    const shown = guarded.getStates();
    expect(shown.has(trapped)).toBe(false);
    expect(shown.has(broken)).toBe(false);
    expect(shown.get(fine)).toEqual(cursor(IN_TEXT, END));
    expect(guarded.states.has(trapped)).toBe(false);
    expect(guarded.states.has(fine)).toBe(true);
  });

  test("the_local_state_is_always_shown", () => {
    const real = tab();
    const guarded = guardedAwareness(real);
    // Ce que y-prosemirror pose pour l'onglet lui-même : des positions de Yjs, pas du JSON.
    const fragment = real.doc.getXmlFragment("document-store");
    const position = Y.createRelativePositionFromTypeIndex(fragment, 0);
    guarded.setLocalStateField("user", MONA);
    guarded.setLocalStateField("cursor", { anchor: position, head: position });

    expect(real.getLocalState()).toEqual({ user: MONA, cursor: { anchor: position, head: position } });
    expect(guarded.getLocalState()).toBe(real.getLocalState());
    expect(guarded.getStates().get(real.clientID)).toBe(real.getLocalState());
    expect(guarded.clientID).toBe(real.clientID);
  });

  test("changes_are_still_announced", () => {
    const real = tab();
    const guarded = guardedAwareness(real);
    const seen: number[][] = [];
    const listener = ({ added }: { added: number[] }) => seen.push(added);
    guarded.on("change", listener);
    const first = receive(real, cursor(END, END));
    expect(seen).toEqual([[first]]);
    guarded.off("change", listener);
    receive(real, cursor(END, END));
    expect(seen).toEqual([[first]]);
  });
});
