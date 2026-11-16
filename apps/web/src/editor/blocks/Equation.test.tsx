// L'équation en bloc : sa formule, mise en forme par KaTeX quand elle est à l'écran, et sa fenêtre,
// où l'on écrit le LaTeX sans rien écrire au document avant « Terminer ». Une formule vient
// d'ailleurs (MMD, un agent, un autre client) : une définition, une imbrication qui ferait planter la
// mise en page, une formule invalide se montrent en texte, avec ce qui les empêche ; KaTeX ne suit
// jamais un lien.
import { NodeSelection } from "prosemirror-state";
import { afterEach, expect, test } from "vitest";
import * as Y from "yjs";
import { containerOf, drag, exchange, peer, REMOTE, seedState, storedProps, stored, type Seed } from "../../test/editors";
import {
  blockOf,
  blockWindow,
  fill,
  flush,
  formulasShown,
  mountEditor,
  press,
  renewSession,
  type Mounted,
} from "../../test/richEditor";
import { until } from "../../test/hooks";
import { FRAME_NODES, resetQueueForTests, setClockForTests } from "./Formula";
import { PAUSE_ABOVE_CHARS } from "./LatexField";
import { MAX_LATEX_CHARS, typeset } from "./tex";

const MODIFY = 'button[aria-label="Modifier l\'équation"]';
const WINDOW = "Formule de l'équation";

function page(latex?: unknown): Seed {
  return [
    { id: "x", type: "paragraph", content: "Avant" },
    { id: "q", type: "equation", props: latex === undefined ? {} : { latex: latex as string } },
    { id: "z", type: "paragraph", content: "Après" },
  ];
}

/** Ouvre la fenêtre de l'équation `q` par son bouton ; rend son champ, qui a le focus. */
async function openWindow(m: Mounted): Promise<HTMLTextAreaElement> {
  blockOf(m, "q").querySelector<HTMLButtonElement>(MODIFY)!.click();
  await flush();
  const field = blockWindow(WINDOW)!.querySelector("textarea")!;
  expect(document.activeElement).toBe(field);
  return field;
}

/** Le bouton « Terminer » de la fenêtre ouverte. */
const finish = () =>
  [...blockWindow(WINDOW)!.querySelectorAll("button")].find((b) => b.textContent === "Terminer")!;

test("equation_is_edited_in_its_window_and_applied_once", async () => {
  const m = await mountEditor({ blocks: page("x") });
  let field = await openWindow(m);
  expect(field.value).toBe("x");
  // Rien ne s'écrit tant qu'on tape ; l'aperçu suit.
  for (const typed of ["x^", "x^2", "x^2 + y"]) {
    fill(field, typed);
    await flush();
  }
  expect(m.writes).toEqual([]);
  expect(blockWindow(WINDOW)!.querySelector(".mm-latex-preview .katex")).not.toBeNull();
  finish().click();
  await flush();
  expect(storedProps(m.doc, "q").latex).toBe("x^2 + y");
  expect(m.writes).toHaveLength(1);
  expect(blockWindow(WINDOW)).toBeNull();
  // Ctrl+Entrée fait de même.
  field = await openWindow(m);
  fill(field, "\\alpha");
  press(field, "Enter", { ctrlKey: true });
  await flush();
  expect(storedProps(m.doc, "q").latex).toBe("\\alpha");
  expect(m.writes).toHaveLength(2);
  expect(document.activeElement).toBe(blockOf(m, "q").querySelector(MODIFY));
});

test("escape_keeps_the_typed_latex", async () => {
  const m = await mountEditor({ blocks: page("x") });
  let field = await openWindow(m);
  fill(field, "\\sqrt{2}");
  press(field, "Escape");
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  expect(storedProps(m.doc, "q").latex).toBe("x");
  field = await openWindow(m);
  expect(field.value).toBe("\\sqrt{2}");
});

test("invalid_latex_is_shown_in_red_and_kept", async () => {
  const m = await mountEditor({ blocks: page("x") });
  const field = await openWindow(m);
  fill(field, "\\frac{1}{");
  await flush();
  const dialog = blockWindow(WINDOW)!;
  expect(dialog.querySelector(".mm-latex-preview .mm-tex-invalid")?.textContent).toBe("\\frac{1}{");
  expect(dialog.querySelector(".mm-block-window-error")?.textContent).toMatch(/^Formule invalide : /);
  expect(field.value).toBe("\\frac{1}{");
  // Appliquée, la formule invalide est gardée, et le bloc la montre en rouge, en texte.
  finish().click();
  await flush();
  expect(storedProps(m.doc, "q").latex).toBe("\\frac{1}{");
  await formulasShown(m);
  const shown = blockOf(m, "q");
  expect(shown.querySelector(".mm-tex-invalid .mm-tex-source")?.textContent).toBe("\\frac{1}{");
  expect(shown.querySelector(".katex")).toBeNull();
});

test("the_field_takes_at_most_ten_thousand_characters", async () => {
  const m = await mountEditor({ blocks: page("x") });
  const field = await openWindow(m);
  expect(field.maxLength).toBe(MAX_LATEX_CHARS);
});

test("the_preview_of_a_long_formula_waits_for_a_pause_in_typing", async () => {
  // Chaque frappe refaisait la mise en forme de toute la formule : 150 ms par frappe à 10 000
  // caractères (relecture de C2, M3). Au-delà de quelques milliers, l'aperçu attend une pause.
  const long = "x".repeat(PAUSE_ABOVE_CHARS + 1);
  const m = await mountEditor({ blocks: page(long) });
  const field = await openWindow(m);
  const previewed = () =>
    blockWindow(WINDOW)!.querySelector('.mm-latex-preview annotation[encoding="application/x-tex"]')?.textContent;
  await until(m.host, () => previewed() === long, "l'aperçu de la formule");
  fill(field, `${long}+y`);
  await flush();
  expect(previewed(), "l'aperçu n'a pas attendu la pause").toBe(long);
  await until(m.host, () => previewed() === `${long}+y`, "l'aperçu après la pause");
  // Témoin : une formule courte suit la frappe.
  fill(field, "a+b");
  await flush();
  expect(previewed()).toBe("a+b");
});

test("katex_never_follows_a_link", async () => {
  const m = await mountEditor({
    blocks: [
      { id: "a", type: "equation", props: { latex: "\\href{javascript:alert(1)}{x}" } },
      { id: "b", type: "equation", props: { latex: "\\url{https://a.example/}" } },
      { id: "c", type: "equation", props: { latex: "\\includegraphics{https://a.example/x.png}" } },
    ],
  });
  await formulasShown(m);
  for (const id of ["a", "b", "c"]) {
    expect(blockOf(m, id).querySelector("a, img, [href], [src]"), id).toBeNull();
  }
  // Témoin : la formule est bien mise en forme (la commande refusée en rouge, par KaTeX).
  expect(blockOf(m, "a").querySelector(".katex")).not.toBeNull();
});

test("empty_equation_says_so", async () => {
  const m = await mountEditor({ blocks: page() });
  expect(blockOf(m, "q").textContent).toContain("Équation vide");
  const blank = await mountEditor({ blocks: page("   ") });
  expect(blockOf(blank, "q").textContent).toContain("Équation vide");
});

test("reader_sees_formulas_and_cannot_open_their_field", async () => {
  const m = await mountEditor({
    blocks: [
      { id: "q", type: "equation", props: { latex: "E = mc^2" } },
      { id: "p", type: "paragraph", content: ["Avec ", { type: "math", props: { latex: "a^2" } }, " en ligne"] },
    ],
    editable: false,
  });
  await formulasShown(m);
  expect(blockOf(m, "q").querySelector(".katex")).not.toBeNull();
  expect(blockOf(m, "p").querySelector(".katex")).not.toBeNull();
  expect(m.host.querySelectorAll("button")).toHaveLength(0);
  // Témoin : modifiable, les deux formules sont des boutons.
  const writer = await mountEditor({
    blocks: [
      { id: "q", type: "equation", props: { latex: "E = mc^2" } },
      { id: "p", type: "paragraph", content: ["Avec ", { type: "math", props: { latex: "a^2" } }, " en ligne"] },
    ],
  });
  expect(blockOf(writer, "q").querySelector(MODIFY)).not.toBeNull();
  expect(blockOf(writer, "p").querySelector('button[aria-label="Modifier la formule"]')).not.toBeNull();
});

test("a_definition_is_shown_as_text_with_why", async () => {
  const m = await mountEditor({ blocks: page("\\def\\a{\\a\\a}\\a") });
  await formulasShown(m);
  const shown = blockOf(m, "q");
  expect(shown.querySelector(".katex")).toBeNull();
  expect(shown.querySelector(".mm-tex-source")?.textContent).toBe("\\def\\a{\\a\\a}\\a");
  expect(shown.querySelector(".mm-tex-why")?.textContent).toBe(
    "Les définitions (\\def, \\newcommand…) ne sont pas permises dans une formule.",
  );
});

test("a_formula_too_nested_for_its_place_is_text_and_a_shallower_one_is_typeset", async () => {
  const frac = (n: number) => "\\frac{1}{".repeat(n) + "x" + "}".repeat(n);
  const m = await mountEditor({
    blocks: [
      { id: "deep", type: "equation", props: { latex: frac(13) } },
      { id: "fine", type: "equation", props: { latex: frac(5) } },
    ],
  });
  await formulasShown(m);
  expect(blockOf(m, "deep").querySelector(".katex")).toBeNull();
  expect(blockOf(m, "deep").querySelector(".mm-tex-why")?.textContent).toBe("Formule trop imbriquée pour être affichée ici.");
  expect(blockOf(m, "fine").querySelector(".katex")).not.toBeNull();
});

test("the_same_formula_is_typeset_at_the_top_and_text_deep_in_a_list", async () => {
  // La part « profondeur de la page » de la garde (relecture de C2, I2) : sans elle, la formule du
  // fond s'attacherait là où elle fait planter Chromium (e2e `a_formula_allowed_at_the_top_is_text_deep_in_a_list`).
  const latex = "\\mathreflectbox{".repeat(18) + "x" + "}".repeat(18);
  type Item = { id: string; type: "bulletListItem"; content: unknown; children?: Item[] };
  let deepest: Item = { id: "l25", type: "bulletListItem", content: ["en ligne ", { type: "math", props: { latex } }] };
  for (let level = 24; level >= 0; level -= 1) {
    deepest = { id: `l${level}`, type: "bulletListItem", content: `niveau ${level}`, children: [deepest] };
  }
  const m = await mountEditor({
    blocks: [{ id: "top", type: "equation", props: { latex } }, deepest] as NonNullable<Parameters<typeof mountEditor>[0]>["blocks"],
  });
  await formulasShown(m);
  expect(blockOf(m, "top").querySelector(".katex")).not.toBeNull();
  expect(blockOf(m, "l25").querySelector(".katex")).toBeNull();
  expect(blockOf(m, "l25").querySelector(".mm-tex-why")?.textContent).toBe("Formule trop imbriquée pour être affichée ici.");
});

// ---------------------------------------------------------------------------
// Rien sur le chemin de l'ouverture : seules les formules à l'écran, l'une après l'autre
// ---------------------------------------------------------------------------

type Observed = { element: Element; callback: IntersectionObserverCallback; observer: IntersectionObserver };
const observed: Observed[] = [];
const originalObserver = globalThis.IntersectionObserver;
const originalFrame = globalThis.requestAnimationFrame;

afterEach(() => {
  globalThis.IntersectionObserver = originalObserver;
  globalThis.requestAnimationFrame = originalFrame;
  setClockForTests(null);
  resetQueueForTests();
  observed.length = 0;
});

/** Un observateur que le test commande : rien n'est à l'écran tant qu'il ne le dit pas. */
function stubObserver(): void {
  globalThis.IntersectionObserver = class {
    constructor(private readonly callback: IntersectionObserverCallback) {}
    observe(element: Element) {
      observed.push({ element, callback: this.callback, observer: this as unknown as IntersectionObserver });
    }
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
}

/** L'élément `element` arrive à l'écran. */
function show(element: Element): void {
  for (const o of observed.filter((x) => x.element === element || x.element.contains(element) || element.contains(x.element))) {
    o.callback([{ isIntersecting: true, target: o.element } as IntersectionObserverEntry], o.observer);
  }
}

test("formulas_are_typeset_only_once_shown", async () => {
  stubObserver();
  const m = await mountEditor({
    blocks: [
      { id: "a", type: "equation", props: { latex: "\\sum_{i=1}^{7} i" } },
      { id: "b", type: "equation", props: { latex: "\\prod_{i=1}^{7} i" } },
    ],
  });
  await flush();
  // Ni l'une ni l'autre n'est à l'écran : leur texte source à leur place, rien de mis en forme.
  for (const id of ["a", "b"]) {
    expect(blockOf(m, id).querySelector(".katex"), id).toBeNull();
    expect(blockOf(m, id).querySelector(".mm-tex-pending .mm-tex-source"), id).not.toBeNull();
  }
  show(blockOf(m, "a"));
  await until(m.host, () => blockOf(m, "a").querySelector(".katex") !== null, "la formule à l'écran mise en forme");
  expect(blockOf(m, "b").querySelector(".katex")).toBeNull();
});

/** Remplace `requestAnimationFrame` : les images demandées attendent que le test les donne. */
function stubFrames(): FrameRequestCallback[] {
  const frames: FrameRequestCallback[] = [];
  globalThis.requestAnimationFrame = ((callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  }) as typeof requestAnimationFrame;
  return frames;
}

/** Donne l'image suivante, puis laisse React rendre. */
async function nextFrame(frames: FrameRequestCallback[], what: string): Promise<void> {
  const frame = frames.shift();
  expect(frame, what).toBeDefined();
  frame!(0);
  await flush();
}

test("formulas_are_typeset_a_few_milliseconds_per_frame", async () => {
  // Une horloge qui avance de 10 ms à chaque lecture : une attache dépasse le budget d'une image
  // (8 ms), la suivante attend l'image suivante. Les formules sont déjà calculées : une formule du
  // cache passe aussi par la file (relecture de C2, C2 : elles s'attachaient toutes au montage).
  const latex = ["\\alpha_{1}", "\\beta_{2}", "\\gamma_{3}"];
  for (const l of latex) typeset(l, true);
  let t = 0;
  setClockForTests(() => (t += 10));
  const frames = stubFrames();
  const m = await mountEditor({
    blocks: latex.map((l, i) => ({ id: `f${i}`, type: "equation" as const, props: { latex: l } })),
  });
  const typesetCount = () => latex.filter((_, i) => blockOf(m, `f${i}`).querySelector(".katex") !== null).length;
  expect(typesetCount()).toBe(0);
  for (const expected of [1, 2, 3]) {
    await nextFrame(frames, `une image demandée pour la formule ${expected}`);
    expect(typesetCount()).toBe(expected);
  }
  expect(frames).toHaveLength(0);
});

test("formulas_are_attached_a_few_thousand_elements_per_frame", async () => {
  // Le coût d'une formule est sa mise en page : une image attache au plus `FRAME_NODES` éléments,
  // et au moins une formule, même plus grande. L'horloge est arrêtée : seul le compte des éléments
  // découpe les images.
  setClockForTests(() => 0);
  const nodes = (latex: string) => {
    const outcome = typeset(latex, true);
    return outcome.kind === "formula" ? outcome.nodes : -1;
  };
  const extra = nodes("x".repeat(100)) - 100;
  const sized = (count: number) => "x".repeat(count - extra);
  const latex = ["a", sized(0.6 * FRAME_NODES), sized(0.6 * FRAME_NODES - 1), "b", sized(1.2 * FRAME_NODES)];
  expect(nodes(latex[1]!)).toBe(0.6 * FRAME_NODES);
  const frames = stubFrames();
  const m = await mountEditor({
    blocks: latex.map((l, i) => ({ id: `f${i}`, type: "equation" as const, props: { latex: l } })),
  });
  const attached = () => latex.map((_, i) => blockOf(m, `f${i}`).querySelector(".katex") !== null);
  expect(attached()).toEqual([false, false, false, false, false]);
  await nextFrame(frames, "la première image");
  expect(attached()).toEqual([true, true, false, false, false]);
  await nextFrame(frames, "la deuxième image");
  expect(attached()).toEqual([true, true, true, true, false]);
  // Plus grande que le budget d'une image : seule dans la sienne.
  await nextFrame(frames, "la troisième image");
  expect(attached()).toEqual([true, true, true, true, true]);
  expect(frames).toHaveLength(0);
});

test("a_formula_computed_past_the_time_of_a_frame_is_attached_at_the_next_one", async () => {
  // Son calcul a pris le temps de l'image : sa mise en page ne s'y ajoute pas.
  let t = 0;
  setClockForTests(() => (t += 10));
  const frames = stubFrames();
  const m = await mountEditor({ blocks: [{ id: "f", type: "equation", props: { latex: "\\zeta_{41} + \\eta" } }] });
  await nextFrame(frames, "l'image du calcul");
  expect(blockOf(m, "f").querySelector(".katex")).toBeNull();
  await nextFrame(frames, "l'image de l'attache");
  expect(blockOf(m, "f").querySelector(".katex")).not.toBeNull();
});

test("a_formula_from_the_cache_waits_for_the_screen", async () => {
  // Au retour sur une page, ses formules sont dans le cache : elles attendent l'écran comme les
  // autres (relecture de C2 : 150 formules s'attachaient d'un coup, 28 s d'onglet figé).
  stubObserver();
  const latex = "\\sum_{k=0}^{9} k^2";
  typeset(latex, true);
  const m = await mountEditor({
    blocks: [
      { id: "a", type: "equation", props: { latex } },
      { id: "b", type: "equation", props: { latex } },
    ],
  });
  await flush();
  for (const id of ["a", "b"]) {
    expect(blockOf(m, id).querySelector(".katex"), id).toBeNull();
    expect(blockOf(m, id).querySelector(".mm-tex-pending .mm-tex-source"), id).not.toBeNull();
  }
  show(blockOf(m, "a"));
  await until(m.host, () => blockOf(m, "a").querySelector(".katex") !== null, "la formule à l'écran attachée");
  expect(blockOf(m, "b").querySelector(".katex")).toBeNull();
});

// ---------------------------------------------------------------------------
// À deux : la fenêtre tenue par l'id du bloc, le champ qui suit la formule tant qu'on n'y tape rien
// ---------------------------------------------------------------------------

async function duo(latex: string) {
  const base = await seedState(page(latex));
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  return { mona, tom, server };
}

test("an_untouched_field_follows_the_formula_and_finishing_writes_nothing", async () => {
  const { mona, tom, server } = await duo("x");
  const field = await openWindow(mona);
  tom.editor.updateBlock("q", { props: { latex: "y^2" } });
  await exchange(server, [tom, mona]);
  await flush();
  expect(field.value).toBe("y^2");
  finish().click();
  await flush();
  expect(mona.writes).toEqual([]);
  await exchange(server, [mona, tom]);
  expect(storedProps(server, "q").latex).toBe("y^2");
});

test("what_was_typed_stays_in_the_field_when_another_moves_or_changes_the_equation", async () => {
  const { mona, tom, server } = await duo("x");
  const field = await openWindow(mona);
  fill(field, "\\int_0^1 f");
  // Tom déplace l'équation (supprimée et récrite au même id), puis en change la formule.
  drag(tom, "q", { under: "z" });
  await exchange(server, [tom, mona]);
  tom.editor.updateBlock("q", { props: { latex: "\\pi" } });
  await exchange(server, [tom, mona]);
  await flush();
  expect(stored(mona.doc).map((b) => b.id)).toEqual(["x", "z", "q"]);
  expect(blockWindow(WINDOW)).not.toBeNull();
  expect(document.activeElement).toBe(field);
  expect(field.value).toBe("\\int_0^1 f");
  fill(field, "\\int_0^1 f(x)");
  await flush();
  expect(mona.writes).toEqual([]);
  finish().click();
  await flush();
  await exchange(server, [mona, tom]);
  expect(storedProps(server, "q").latex).toBe("\\int_0^1 f(x)");
});

test("ctrl_alt_enter_opens_the_window_of_a_selected_equation", async () => {
  const m = await mountEditor({ blocks: page("x^2") });
  m.editor.focus();
  m.editor.transact((tr) => tr.setSelection(NodeSelection.create(tr.doc, containerOf(tr.doc, "q").pos + 1)));
  const event = new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
  m.editor.prosemirrorView.dom.dispatchEvent(event);
  await flush();
  expect(event.defaultPrevented).toBe(true);
  expect(blockWindow(WINDOW)?.querySelector("textarea")?.value).toBe("x^2");
});

test("a_formula_typed_before_losing_the_right_stays_within_reach", async () => {
  // La page passe en lecture seule (une session neuve) : la formule tapée reste dans la fenêtre, en
  // lecture seule, et, la fenêtre fermée, le bouton du bloc la rouvre (R472).
  const m = await mountEditor({ blocks: page("x"), pageId: "page-eq" });
  fill(await openWindow(m), "\beta");
  await renewSession(m, { editable: false, pageId: "page-eq" });
  const dialog = blockWindow(WINDOW)!;
  expect(dialog.querySelector("textarea")!.readOnly).toBe(true);
  expect(dialog.querySelector('[role="status"]')?.textContent).toContain("Tu ne peux pas modifier cette page");
  press(dialog.querySelector("textarea")!, "Escape");
  await flush();
  const button = blockOf(m, "q").querySelector<HTMLButtonElement>('button[aria-label="Formule non enregistrée de l\'équation"]');
  expect(document.activeElement).toBe(button);
  button!.click();
  await flush();
  expect(blockWindow(WINDOW)!.querySelector("textarea")!.value).toBe("\beta");
});
