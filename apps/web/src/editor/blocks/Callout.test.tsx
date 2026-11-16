// Le callout : un encadré, son icône et sa couleur, réglées dans sa fenêtre (le bouton de l'icône,
// ou Ctrl+Alt+Entrée depuis son texte). L'icône et la couleur viennent du document, donc d'ailleurs
// (MMD, un agent, un autre client) : l'icône est du texte, jamais du HTML, et la couleur ne choisit
// qu'une classe d'une liste fermée, jamais un attribut `style`.
import { expect, test } from "vitest";
import themeCss from "../../theme.css?raw";
import { contrast } from "../../page/avatarInk";
import { storedProps } from "../../test/editors";
import { blockOf, flush, foreignStyles, mountEditor } from "../../test/richEditor";
import { CALLOUT_BACKGROUNDS, CALLOUT_COLORS, CALLOUT_ICONS } from "./Callout";

/** Un callout entre deux paragraphes. */
function page(props: Record<string, unknown> = {}) {
  return [
    { id: "x", type: "paragraph" as const, content: "Avant" },
    { id: "c", type: "callout" as const, props, content: "Attention" },
    { id: "z", type: "paragraph" as const, content: "Après" },
  ];
}

const ICON_BUTTON = 'button[aria-label="Icône et couleur du callout"]';
const WINDOW = '[role="dialog"][aria-label="Icône et couleur du callout"]';

function key(target: Element, key: string) {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

test("the_rich_lists_are_those_of_the_plan", () => {
  expect(CALLOUT_ICONS).toEqual(["💡", "⚠️", "❗", "✅", "❓", "📌", "🔥", "📝"]);
  expect(CALLOUT_COLORS).toEqual([
    "default",
    "gray",
    "brown",
    "red",
    "orange",
    "yellow",
    "green",
    "blue",
    "purple",
    "pink",
  ]);
});

test("callout_icon_and_color_are_chosen_from_the_keyboard", async () => {
  const m = await mountEditor({ blocks: page() });
  const button = blockOf(m, "c").querySelector<HTMLButtonElement>(ICON_BUTTON)!;
  expect(button, "le bouton de l'icône").not.toBeNull();
  expect(button.getAttribute("aria-expanded")).toBe("false");
  button.click();
  await flush();
  const dialog = document.querySelector<HTMLElement>(WINDOW)!;
  expect(dialog, "la fenêtre du callout").not.toBeNull();
  expect(blockOf(m, "c").querySelector(ICON_BUTTON)!.getAttribute("aria-expanded")).toBe("true");

  // Le focus est sur l'icône du bloc ; les flèches parcourent sans rien écrire, Entrée choisit.
  const icons = dialog.querySelector<HTMLElement>('ul[role="listbox"][aria-label="Icône"]')!;
  expect(document.activeElement?.textContent).toBe("💡");
  expect(document.activeElement?.getAttribute("aria-selected")).toBe("true");
  key(document.activeElement!, "ArrowRight");
  await flush();
  expect(document.activeElement?.textContent).toBe("⚠️");
  expect(storedProps(m.doc, "c").icon ?? "💡").toBe("💡");
  key(document.activeElement!, "Enter");
  await flush();
  expect(storedProps(m.doc, "c").icon).toBe("⚠️");
  expect(icons.querySelector('[aria-selected="true"]')?.textContent).toBe("⚠️");

  // La couleur : la liste suivante, Début, Fin et les flèches.
  const colors = dialog.querySelector<HTMLElement>('ul[role="listbox"][aria-label="Couleur"]')!;
  const current = colors.querySelector<HTMLElement>('[tabindex="0"]')!;
  expect(current.textContent).toBe("Par défaut");
  current.focus();
  key(current, "End");
  await flush();
  expect(document.activeElement?.textContent).toBe("Rose");
  key(document.activeElement!, "ArrowUp");
  key(document.activeElement!, "ArrowUp");
  await flush();
  expect(document.activeElement?.textContent).toBe("Bleu");
  key(document.activeElement!, "Enter");
  await flush();
  expect(storedProps(m.doc, "c")).toMatchObject({ icon: "⚠️", color: "blue" });
  // Le texte du callout n'a pas bougé.
  expect(m.editor.getBlock("c")?.content).toEqual([{ type: "text", text: "Attention", styles: {} }]);

  // Échap ferme la fenêtre et rend le focus au bouton de l'icône.
  key(document.activeElement!, "Escape");
  await flush();
  expect(document.querySelector(WINDOW)).toBeNull();
  expect(document.activeElement).toBe(blockOf(m, "c").querySelector(ICON_BUTTON));
});

test("callout_text_contrast_is_at_least_4_5", () => {
  for (const color of CALLOUT_COLORS) {
    expect(contrast("#2F2E2B", CALLOUT_BACKGROUNDS[color]), color).toBeGreaterThanOrEqual(4.5);
  }
});

test("the_backgrounds_of_the_contrast_test_are_those_of_the_theme", () => {
  // Le fond affiché est dans `theme.css`, par classe : le test de contraste doit lire le même.
  for (const color of CALLOUT_COLORS) {
    const rule = new RegExp(`\\.mm-callout-${color}\\s*\\{\\s*background:\\s*(#[0-9a-fA-F]{6});`);
    const found = themeCss.match(rule);
    expect(found?.[1]?.toUpperCase(), `.mm-callout-${color} dans theme.css`).toBe(
      CALLOUT_BACKGROUNDS[color].toUpperCase(),
    );
  }
});

test("the_icon_names_the_emoji_fonts_after_those_of_the_text", () => {
  // Une icône est le plus souvent un emoji : nommer les polices d'emoji évite à Chromium de les
  // chercher dans celles du système à chaque ouverture (R629) ; après celles du texte, pour qu'une
  // lettre garde la police de l'application.
  const rule = themeCss.match(/\.mm-callout-glyph\s*\{([^}]*)\}/)?.[1] ?? "";
  expect(rule).toMatch(
    /font-family:\s*var\(--mm-font\),\s*"Apple Color Emoji",\s*"Segoe UI Emoji",\s*"Noto Color Emoji";/,
  );
});

test("an_icon_from_elsewhere_is_shown_as_text", async () => {
  const tag = '<img src="x" onerror="alert(1)">';
  const long = "Une icône bien trop longue, écrite par MMD.";
  const m = await mountEditor({
    blocks: [
      { id: "t", type: "callout", props: { icon: tag }, content: "Balise" },
      { id: "l", type: "callout", props: { icon: long }, content: "Longue" },
      { id: "e", type: "callout", props: { icon: "⚠️" }, content: "Emoji" },
    ],
  });
  const tagged = blockOf(m, "t");
  expect(tagged.querySelector("img")).toBeNull();
  const glyph = (id: string) => blockOf(m, id).querySelector<HTMLElement>(".mm-callout-glyph")!;
  expect(glyph("t").textContent).toBe("<im…");
  expect(glyph("l").textContent).toBe("Une…");
  // L'icône entière se lit dans l'infobulle, isolée.
  expect(glyph("l").title).toBe(`⁨${long}⁩`);
  expect(glyph("e").textContent).toBe("⚠️");
  expect(glyph("e").title).toBe("");
  for (const id of ["t", "l", "e"]) expect(glyph(id).getAttribute("dir")).toBe("auto");
});

/** Des couleurs et des icônes qu'aucun client honnête n'écrit. */
const HOSTILE = [
  { color: "red; background: url(https://ailleurs.example/x.png)", icon: "💡" },
  { color: "__proto__", icon: "x" },
  { color: "constructor", icon: "y" },
  { color: "toString", icon: "z" },
  { color: "Blue", icon: "a; color: red" },
  { color: "var(--mm-danger)", icon: "b" },
];

test("callout_never_takes_a_style_from_the_document", async () => {
  const m = await mountEditor({
    blocks: [
      ...HOSTILE.map((props, i) => ({ id: `h${i}`, type: "callout" as const, props, content: `Hostile ${i}` })),
      { id: "known", type: "callout" as const, props: { color: "blue" }, content: "Connu" },
    ],
  });
  for (let i = 0; i < HOSTILE.length; i += 1) {
    const element = blockOf(m, `h${i}`);
    // Aucun élément du bloc ne porte d'attribut `style` à lui : la couleur passe par une classe.
    expect(foreignStyles(element), JSON.stringify(HOSTILE[i])).toEqual([]);
    const callout = element.querySelector(".mm-callout")!;
    // Une couleur hors de la liste est celle par défaut.
    expect([...callout.classList].filter((c) => c.startsWith("mm-callout-")), JSON.stringify(HOSTILE[i])).toEqual([
      "mm-callout-default",
    ]);
  }
  // Témoin : une couleur connue choisit sa classe, et rien de plus.
  const known = blockOf(m, "known");
  expect(known.querySelector(".mm-callout")!.classList.contains("mm-callout-blue")).toBe(true);
  expect(foreignStyles(known)).toEqual([]);
});

test("callout_has_no_icon_button_for_a_reader", async () => {
  const m = await mountEditor({ blocks: page({ icon: "⚠️" }), editable: false });
  const element = blockOf(m, "c");
  expect(element.querySelector("button")).toBeNull();
  expect(element.querySelector(".mm-callout-glyph")?.textContent).toBe("⚠️");
  // Témoin : le même callout, dans un éditeur modifiable, a son bouton.
  const writer = await mountEditor({ blocks: page({ icon: "⚠️" }) });
  expect(blockOf(writer, "c").querySelector(ICON_BUTTON)).not.toBeNull();
});

test("an_editor_that_becomes_editable_shows_the_icon_button", async () => {
  // L'éditeur de la page est verrouillé jusqu'à la première synchro, puis déverrouillé sans que la
  // vue soit remontée (`Editor.tsx`) : le bouton doit paraître alors.
  const m = await mountEditor({ blocks: page(), editable: false });
  expect(blockOf(m, "c").querySelector(ICON_BUTTON)).toBeNull();
  m.editor.isEditable = true;
  await flush();
  expect(blockOf(m, "c").querySelector(ICON_BUTTON)).not.toBeNull();
});

test("an_icon_that_shows_nothing_falls_back_to_the_default_icon", async () => {
  const m = await mountEditor({
    blocks: [
      { id: "v", type: "callout", props: { icon: "" }, content: "Vide" },
      { id: "f", type: "callout", props: { icon: "\u202E\u202E\u202E" }, content: "Format" },
      { id: "w", type: "callout", props: { icon: "  \u200B " }, content: "Blanc" },
      // Sondes de la re-relecture : remplissage hangûl, Braille vide, sélecteur de variante seul,
      // graphème combinant, remplissage de l'initiale hangûl.
      { id: "h", type: "callout", props: { icon: "\u3164\u3164" }, content: "Hangûl" },
      { id: "b", type: "callout", props: { icon: "\u2800" }, content: "Braille" },
      { id: "s", type: "callout", props: { icon: "\uFE0F" }, content: "Sélecteur" },
      { id: "g", type: "callout", props: { icon: "\u034F\u034F" }, content: "Combinant" },
      { id: "i", type: "callout", props: { icon: "\u115F" }, content: "Initiale" },
      { id: "m", type: "callout", props: { icon: "\u0301\u0301" }, content: "Marques" },
      { id: "e", type: "callout", props: { icon: "⚠️" }, content: "Témoin" },
      // Témoin : une lettre et son accent combinant se voient.
      { id: "a", type: "callout", props: { icon: "e\u0301" }, content: "Accent" },
    ],
  });
  expect(blockOf(m, "a").querySelector(".mm-callout-glyph")?.textContent).toBe("e\u0301");
  for (const id of ["v", "f", "w", "h", "b", "s", "g", "i", "m"]) {
    expect(blockOf(m, id).querySelector(".mm-callout-glyph"), id).toBeNull();
    expect(blockOf(m, id).querySelector(".mm-callout-icon svg"), id).not.toBeNull();
  }
  // Témoin : une vraie icône se montre en texte.
  expect(blockOf(m, "e").querySelector(".mm-callout-glyph")?.textContent).toBe("⚠️");
});
