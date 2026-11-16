// Le signet : une carte qui montre l'hôte et l'adresse d'une page web, sans aperçu distant (R250).
// Son adresse ne s'écrit que par sa fenêtre (« Modifier le signet »), et seulement si c'est une
// adresse web entière (`webUrl`) ; ce que le document porte d'autre (MMD, un autre client) se montre
// en texte, jamais en lien.
import { expect, test } from "vitest";
import { contrast } from "../../page/avatarInk";
import * as Y from "yjs";
import { exchange, peer, REMOTE, seedState, storedProps } from "../../test/editors";
import { blockOf, blockWindow, fill, flush, foreignStyles, mountEditor, press } from "../../test/richEditor";

const MODIFY = 'button[aria-label="Modifier le signet"]';
const WINDOW = "Adresse du signet";
const REFUSED = "Une adresse commence par http:// ou https://";

function page(url?: unknown) {
  return [
    { id: "x", type: "paragraph" as const, content: "Avant" },
    { id: "s", type: "bookmark" as const, props: url === undefined ? {} : { url: url as string } },
    { id: "z", type: "paragraph" as const, content: "Après" },
  ];
}

/** Ouvre la fenêtre du signet `s` par son bouton ; rend son champ, qui a le focus. */
async function openWindow(m: Awaited<ReturnType<typeof mountEditor>>): Promise<HTMLInputElement> {
  blockOf(m, "s").querySelector<HTMLButtonElement>(MODIFY)!.click();
  await flush();
  const dialog = blockWindow(WINDOW);
  expect(dialog, "la fenêtre du signet").not.toBeNull();
  const field = dialog!.querySelector<HTMLInputElement>("input")!;
  expect(document.activeElement).toBe(field);
  return field;
}

async function submit(field: HTMLInputElement): Promise<void> {
  field.form!.requestSubmit();
  await flush();
}

test("bookmark_takes_only_a_web_address", async () => {
  const m = await mountEditor({ blocks: page() });
  const field = await openWindow(m);
  expect(field.value).toBe("");
  for (const raw of ["javascript:alert(1)", "data:text/html,<script>alert(1)</script>", "//hôte", "exemple.fr"]) {
    fill(field, raw);
    await flush();
    await submit(field);
    const error = blockWindow(WINDOW)!.querySelector('[role="alert"]');
    expect(error?.textContent, raw).toBe(REFUSED);
    expect(field.getAttribute("aria-invalid"), raw).toBe("true");
    expect(field.getAttribute("aria-describedby"), raw).toBe(error!.id);
    // Rien n'est écrit, et la fenêtre reste, avec le texte tapé.
    expect(storedProps(m.doc, "s").url ?? "", raw).toBe("");
    expect(m.writes, raw).toEqual([]);
    expect(field.value, raw).toBe(raw);
    expect(document.activeElement, raw).toBe(field);
  }
  // Une frappe retire le message.
  fill(field, "https://exemple.fr/a");
  await flush();
  expect(blockWindow(WINDOW)!.querySelector('[role="alert"]')).toBeNull();
  await submit(field);
  expect(storedProps(m.doc, "s").url).toBe("https://exemple.fr/a");
  // Écrite, la fenêtre se ferme et rend le focus au bouton du signet.
  expect(blockWindow(WINDOW)).toBeNull();
  expect(document.activeElement).toBe(blockOf(m, "s").querySelector(MODIFY));
});

test("the_written_address_is_the_one_webUrl_gives", async () => {
  const m = await mountEditor({ blocks: page() });
  const field = await openWindow(m);
  fill(field, "  https://Mementum.dev  ");
  await submit(field);
  expect(storedProps(m.doc, "s").url).toBe("https://mementum.dev/");
});

test("escape_keeps_the_typed_address", async () => {
  const m = await mountEditor({ blocks: page("https://mementum.dev/") });
  let field = await openWindow(m);
  expect(field.value).toBe("https://mementum.dev/");
  fill(field, "https://mementum.dev/doc");
  press(field, "Escape");
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  expect(document.activeElement).toBe(blockOf(m, "s").querySelector(MODIFY));
  expect(storedProps(m.doc, "s").url).toBe("https://mementum.dev/");
  // Rouverte, la fenêtre retrouve le texte tapé.
  field = await openWindow(m);
  expect(field.value).toBe("https://mementum.dev/doc");
});

test("bookmark_shows_host_and_address_as_text", async () => {
  const m = await mountEditor({ blocks: page("https://mementum.dev/docs?x=<b>1</b>") });
  const card = blockOf(m, "s");
  const link = card.querySelector("a")!;
  expect(link.getAttribute("href")).toBe("https://mementum.dev/docs?x=%3Cb%3E1%3C/b%3E");
  expect(link.getAttribute("target")).toBe("_blank");
  expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  const host = card.querySelector(".mm-bookmark-host")!;
  const address = card.querySelector(".mm-bookmark-url")!;
  expect(host.textContent).toBe("mementum.dev");
  expect(address.textContent).toBe("https://mementum.dev/docs?x=%3Cb%3E1%3C/b%3E");
  expect(host.getAttribute("dir")).toBe("auto");
  expect(address.getAttribute("dir")).toBe("auto");
  expect(card.querySelector("b")).toBeNull();
  expect(foreignStyles(card)).toEqual([]);
});

test("an_address_that_is_not_a_web_address_is_text_not_a_link", async () => {
  for (const url of ["javascript:alert(1)", "data:text/html,x", "//hôte", "<img src=x onerror=alert(1)>"]) {
    const m = await mountEditor({ blocks: page(url) });
    const card = blockOf(m, "s");
    expect(card.querySelector("a"), url).toBeNull();
    expect(card.querySelector("img"), url).toBeNull();
    expect(card.querySelector(".mm-bookmark-raw")?.textContent, url).toBe(url);
    expect(card.querySelector(".mm-bookmark-raw")?.getAttribute("dir"), url).toBe("auto");
  }
  const empty = await mountEditor({ blocks: page() });
  expect(blockOf(empty, "s").textContent).toContain("Signet sans adresse");
});

test("bookmark_has_no_modify_button_for_a_reader", async () => {
  const m = await mountEditor({ blocks: page("https://mementum.dev/"), editable: false });
  expect(blockOf(m, "s").querySelector("button")).toBeNull();
  expect(blockOf(m, "s").querySelector("a")).not.toBeNull();
  // Témoin : modifiable, le bouton est là.
  const writer = await mountEditor({ blocks: page("https://mementum.dev/") });
  expect(blockOf(writer, "s").querySelector(MODIFY)).not.toBeNull();
});

test("the_button_that_closes_the_window_keeps_the_focus", async () => {
  // Un clic ne donne pas le focus à un bouton dans tous les navigateurs (Safari, jsdom) : la fenêtre
  // qui avait le focus partie, il tombait sur `body`.
  const m = await mountEditor({ blocks: page("https://mementum.dev/") });
  await openWindow(m);
  const button = blockOf(m, "s").querySelector<HTMLButtonElement>(MODIFY)!;
  button.click();
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  expect(document.activeElement).toBe(blockOf(m, "s").querySelector(MODIFY));
});

test("the_colours_of_the_rich_blocks_are_readable", () => {
  // Le texte secondaire du signet sur son cadre, et les textes de la fenêtre sur leur fond.
  expect(contrast("#66655F", "#FAFAF8")).toBeGreaterThanOrEqual(4.5);
  expect(contrast("#66655F", "#FFFFFF")).toBeGreaterThanOrEqual(4.5);
  expect(contrast("#B4462B", "#FFFFFF")).toBeGreaterThanOrEqual(4.5);
  expect(contrast("#8A5A0B", "#FCF4E3")).toBeGreaterThanOrEqual(4.5);
  // Le texte d'un choix retenu, sur le fond d'accent (#23704F à 13 %, posé sur du blanc).
  expect(contrast("#2F2E2B", "#E1ECE7")).toBeGreaterThanOrEqual(4.5);
});

test("an_untouched_field_follows_the_address_and_saving_it_writes_nothing", async () => {
  // Mona ouvre la fenêtre et ne tape rien ; Tom change l'adresse ; Mona fait Entrée.
  const base = await seedState(page("https://mementum.dev/"));
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  const field = await openWindow(mona);
  // Tom écrit une adresse telle que MMD l'écrit, que `webUrl` récrirait (`…/` en plus).
  tom.editor.updateBlock("s", { props: { url: "https://tom.example" } });
  await exchange(server, [tom, mona]);
  await flush();
  // Le champ suit l'adresse du bloc tant qu'on n'y a rien tapé.
  expect(field.value).toBe("https://tom.example");
  await submit(field);
  expect(blockWindow(WINDOW)).toBeNull();
  // Rien n'est écrit : l'adresse de Tom reste, telle quelle, chez tous.
  expect(mona.writes).toEqual([]);
  await exchange(server, [mona, tom]);
  expect(storedProps(server, "s").url).toBe("https://tom.example");
});

test("a_touched_field_keeps_what_was_typed_when_another_changes_the_address", async () => {
  const base = await seedState(page("https://mementum.dev/"));
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  const field = await openWindow(mona);
  fill(field, "https://mona.example/");
  tom.editor.updateBlock("s", { props: { url: "https://tom.example/" } });
  await exchange(server, [tom, mona]);
  await flush();
  // Ce qu'on a tapé n'est jamais remplacé ; l'enregistrer est un choix.
  expect(field.value).toBe("https://mona.example/");
  await submit(field);
  await exchange(server, [mona, tom]);
  expect(storedProps(server, "s").url).toBe("https://mona.example/");
});
