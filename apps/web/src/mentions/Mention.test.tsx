// La mention dans le texte (Task C4, R251, R615) : une page, une personne ou une date, avec le
// libellé copié dans le document quand on l'a posée. Qui voit la page mentionnée (elle est dans
// l'arbre de l'espace affiché) lit son titre à jour ; qui ne la voit pas ne lit que le libellé copié,
// jamais un titre venu d'ailleurs (preuve 4 du plan).
import { NodeSelection } from "prosemirror-state";
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { EditorView } from "../editor/Editor";
import { contrast } from "../page/avatarInk";
import { useLinkInterception } from "../router";
import { publishPages } from "../space/pageDirectory";
import { type Seed } from "../test/editors";
import { page } from "../test/pages";
import { blockOf, foreignStyles, mountEditor, press, type Mounted } from "../test/richEditor";

const PLAN = "0190f5a0-0000-7000-8000-0000000000a1";
const NOTES = "0190f5a0-0000-7000-8000-0000000000a2";

afterEach(() => {
  publishPages(null, null);
  history.replaceState(null, "", "/");
});

/** Une page dont le bloc `y` porte une mention entre deux textes. */
function withMention(kind: unknown, id: unknown, label: unknown): Seed {
  return [
    {
      id: "y",
      type: "paragraph",
      content: [
        "Voir ",
        { type: "mention", props: { kind: kind as string, id: id as string, label: label as string } },
        " demain",
      ],
    },
  ];
}

/** Ce que montre la mention du bloc `y` : son texte, et l'adresse de son lien si elle en est un. */
function shownMention(m: Mounted): { text: string | null; href: string | null } {
  const view = blockOf(m, "y").querySelector(".mm-mention");
  const anchor = view?.closest("a") ?? view?.querySelector("a") ?? null;
  return { text: view?.textContent ?? null, href: anchor?.getAttribute("href") ?? null };
}

const publish = (...args: Parameters<typeof publishPages>) => act(async () => publishPages(...args));

/** Les attributs de la mention du document de `m`. */
function storedMention(m: Mounted): Record<string, unknown> {
  let attrs: Record<string, unknown> = {};
  m.editor.prosemirrorState.doc.descendants((node) => {
    if (node.type.name === "mention") attrs = node.attrs;
  });
  return attrs;
}

test("page_mention_shows_the_current_title_only_to_who_sees_the_page", async () => {
  // L'arbre du lecteur porte d'autres pages, dont une au titre que la page mentionnée a pris depuis.
  await publish("space", [page(NOTES, { title: "Plan 2027" })]);
  const m = await mountEditor({ blocks: withMention("page", PLAN, "Plan") });
  expect(shownMention(m)).toEqual({ text: "@Plan", href: `/p/${PLAN}` });
  expect(m.text()).not.toContain("2027");
  // Témoin : la même page publiée, son titre à jour paraît, sans rien écrire dans le document.
  const before = m.writes.length;
  await publish("space", [page(NOTES, { title: "Notes" }), page(PLAN, { title: "Plan 2027" })]);
  expect(shownMention(m).text).toBe("@Plan 2027");
  expect(m.writes.length).toBe(before);
  expect(storedMention(m)).toMatchObject({ kind: "page", id: PLAN, label: "Plan" });
  // L'accès retiré (la page quitte l'arbre) : le libellé copié revient.
  await publish("space", [page(NOTES, { title: "Notes" })]);
  expect(shownMention(m).text).toBe("@Plan");
  // Un libellé vide : « Sans titre ».
  const empty = await mountEditor({ blocks: withMention("page", PLAN, "") });
  expect(shownMention(empty).text).toBe("@Sans titre");
});

test("page_mention_without_a_page_is_not_a_link", async () => {
  // Des pages publiées sous des ids qui n'en sont pas : leur titre ne se lit jamais.
  await publish("space", [page("plan", { title: "Secret" }), page("", { title: "Vide" }), page(PLAN, { title: "Plan 2027" })]);
  for (const id of ["", "plan", "javascript:alert(1)", "../settings", `${PLAN}/../x`, `/p/${PLAN}`, 12, null]) {
    const m = await mountEditor({ blocks: withMention("page", id, "Plan") });
    expect(shownMention(m), String(id)).toEqual({ text: "@Plan", href: null });
    expect(blockOf(m, "y").querySelector("a"), String(id)).toBeNull();
  }
  // Témoin : un identifiant de page fait un lien, vers la page et nulle part ailleurs.
  const linked = await mountEditor({ blocks: withMention("page", PLAN, "Plan") });
  expect(shownMention(linked)).toEqual({ text: "@Plan 2027", href: `/p/${PLAN}` });
});

test("mention_label_is_text_isolated", async () => {
  const tag = '<img src="x" onerror="alert(1)"><b>gras</b>';
  for (const [kind, id] of [
    ["page", PLAN],
    ["page", "pas-un-id"],
    ["user", "u-tom"],
    ["date", "illisible"],
    ["rappel", "x"],
  ]) {
    const m = await mountEditor({ blocks: withMention(kind, id, tag) });
    const view = blockOf(m, "y").querySelector(".mm-mention")!;
    // Du texte, jamais du HTML ni un attribut `style`.
    expect(view.textContent, kind).toBe(`@${tag}`);
    expect(view.querySelector("img, b"), kind).toBeNull();
    expect(foreignStyles(blockOf(m, "y")), kind).toEqual([]);
    // Isolé du texte voisin : un libellé de droite à gauche ne retourne pas la ligne.
    const label = view.querySelector(".mm-mention-label")!;
    expect(label.getAttribute("dir"), kind).toBe("auto");
    expect(label.textContent, kind).toBe(tag);
    expect(m.errors, kind).toEqual([]);
  }
  // Un libellé démesuré est coupé à l'affichage ; le document le garde.
  const long = await mountEditor({ blocks: withMention("user", "u-tom", "é".repeat(600)) });
  expect(shownMention(long).text).toBe(`@${"é".repeat(500)}…`);
  // Une prop qui n'est pas un texte (un autre client, un agent) ne casse rien.
  const odd = await mountEditor({ blocks: withMention({ a: 1 }, ["x"], 12) });
  expect(shownMention(odd)).toEqual({ text: "@…", href: null });
  expect(odd.errors).toEqual([]);
});

test("date_mention_is_formatted_from_its_id_or_shown_as_written", async () => {
  // L'id fait foi : le libellé copié ne sert que s'il est illisible.
  const dated = await mountEditor({ blocks: withMention("date", "2026-12-01", "n'importe quoi") });
  expect(shownMention(dated)).toEqual({ text: "@1 déc. 2026", href: null });
  for (const id of ["demain", "2026-02-30", "01/12/2026", "2026-12-1", ""]) {
    const m = await mountEditor({ blocks: withMention("date", id, "le jour J") });
    expect(shownMention(m).text, id).toBe("@le jour J");
  }
  // Ni l'un ni l'autre : l'id tel qu'il est écrit, ou « … ».
  const bare = await mountEditor({ blocks: withMention("date", "bientôt", "") });
  expect(shownMention(bare).text).toBe("@bientôt");
  const nothing = await mountEditor({ blocks: withMention("date", "", "") });
  expect(shownMention(nothing).text).toBe("@…");
});

test("a_person_mention_shows_the_copied_name_and_is_never_a_link", async () => {
  // Une page publiée sous l'id de la personne : une mention de personne ne lit pas l'arbre.
  await publish("space", [page(PLAN, { title: "Plan 2027" })]);
  const m = await mountEditor({ blocks: withMention("user", PLAN, "Tom") });
  expect(shownMention(m)).toEqual({ text: "@Tom", href: null });
  const unnamed = await mountEditor({ blocks: withMention("user", "u-tom", "") });
  expect(shownMention(unnamed).text).toBe("@Sans nom");
  // Une sorte inconnue (un client plus récent, MMD) : le libellé, sans lien.
  const other = await mountEditor({ blocks: withMention("rappel", PLAN, "Demain 9 h") });
  expect(shownMention(other)).toEqual({ text: "@Demain 9 h", href: null });
});

/** L'application, réduite à ce qui suit les liens internes, autour de l'éditeur. */
function Following({ children }: { children: React.ReactNode }) {
  useLinkInterception();
  return <>{children}</>;
}

test("a_click_on_a_page_mention_opens_the_page_without_reloading", async () => {
  for (const editable of [true, false]) {
    history.replaceState(null, "", "/");
    const m = await mountEditor({
      blocks: withMention("page", PLAN, "Plan"),
      view: (editor) => (
        <Following>
          <EditorView editor={editor} editable={editable} />
        </Following>
      ),
    });
    const anchor = blockOf(m, "y").querySelector("a")!;
    // Un îlot non modifiable : c'est là, et là seulement, que l'application suit un lien de l'éditeur.
    expect(anchor.closest("[contenteditable]")?.getAttribute("contenteditable"), String(editable)).toBe("false");
    const click = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
    act(() => {
      anchor.dispatchEvent(click);
    });
    expect(click.defaultPrevented, String(editable)).toBe(true);
    expect(location.pathname, String(editable)).toBe(`/p/${PLAN}`);
    // Un clic qui ouvre ailleurs (Ctrl) est laissé au navigateur.
    history.replaceState(null, "", "/");
    const elsewhere = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ctrlKey: true });
    // jsdom ne navigue pas ; on l'empêche de s'en plaindre.
    anchor.addEventListener("click", (event) => event.preventDefault(), { once: true });
    act(() => {
      anchor.dispatchEvent(elsewhere);
    });
    expect(location.pathname, String(editable)).toBe("/");
  }
});

/** Sélectionne la mention du bloc `y`, comme les flèches. */
function selectMention(m: Mounted): void {
  m.editor.transact((tr) => {
    let at = -1;
    tr.doc.descendants((node, pos) => {
      if (node.type.name === "mention") at = pos;
    });
    tr.setSelection(NodeSelection.create(tr.doc, at));
  });
}

test("ctrl_alt_enter_on_a_selected_page_mention_puts_the_focus_on_its_link", async () => {
  const m = await mountEditor({ blocks: withMention("page", PLAN, "Plan") });
  act(() => {
    selectMention(m);
    m.editor.focus();
  });
  const event = press(m.editor.prosemirrorView.dom, "Enter", { ctrlKey: true, altKey: true });
  expect(event.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(blockOf(m, "y").querySelector("a"));
  // Témoin : une mention qui n'est pas un lien laisse passer la touche, et le focus où il est.
  const person = await mountEditor({ blocks: withMention("user", "u-tom", "Tom") });
  act(() => {
    selectMention(person);
    person.editor.focus();
  });
  const passed = press(person.editor.prosemirrorView.dom, "Enter", { ctrlKey: true, altKey: true });
  expect(passed.defaultPrevented).toBe(false);
});

test("the_colours_of_a_mention_are_readable", () => {
  // L'accent sur le fond de la page, et sur celui d'une surface (une citation, un en-tête de tableau).
  expect(contrast("#23704F", "#FFFFFF")).toBeGreaterThanOrEqual(4.5);
  expect(contrast("#23704F", "#F4F3EF")).toBeGreaterThanOrEqual(4.5);
});
