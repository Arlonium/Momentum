// Les menus `@` et `[[` (Task C4, R251) : ce qu'ils proposent, à qui, et ce qu'ils posent dans le
// document. Aucun ne propose une page que le compte ne voit pas : les pages viennent de l'arbre de
// l'espace affiché, déjà filtré par les droits (`pageDirectory.ts`).
import type { Member, PageNode } from "@mementum/protocol";
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { ApiError, createPage, getMembers } from "../api";
import type { RosterUser } from "../editor/collab";
import { EditorView } from "../editor/Editor";
import { publishPages } from "../space/pageDirectory";
import { cursor, exchange, peer, shownInline, typeAt, typeAtEnd, type Seed } from "../test/editors";
import { deferred, until } from "../test/hooks";
import { page, unauthenticated } from "../test/pages";
import { flush, mountEditor, press, type Mounted } from "../test/richEditor";
import { createdNotPlaced, createRefused, type MentionContext } from "./MentionMenu";
import { forgetPeople } from "./people";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMembers: vi.fn(),
  createPage: vi.fn(),
}));

const members = vi.mocked(getMembers);
const creates = vi.mocked(createPage);

const HERE = "0190f5a0-0000-7000-8000-00000000000f";
const pageId = (n: number) => `0190f5a0-0000-7000-8000-${String(n).padStart(12, "0")}`;

const member = (userId: string, name: string, role: Member["role"] = "member"): Member => ({
  userId,
  name,
  email: `${userId}@exemple.fr`,
  role,
});

const ROSTER: RosterUser[] = [
  { id: "u-mona", name: "Mona", canWrite: true },
  { id: "u-ines", name: "Inès", canWrite: true },
];

const PAGE: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "paragraph", content: "Voir " },
  { id: "c", type: "codeBlock", content: "code " },
];

/** jsdom n'a pas de géométrie : le menu de BlockNote se place sur un rectangle qu'il sérialise. */
const RECT = { x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0 };
const realRect = Element.prototype.getBoundingClientRect;

beforeEach(() => {
  Element.prototype.getBoundingClientRect = () => ({ ...RECT, toJSON: () => RECT }) as DOMRect;
  // Le lundi 16 novembre 2026 ; seule la date est figée, pas les minuteurs.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 16, 10, 0));
  members.mockReset();
  creates.mockReset();
  members.mockResolvedValue([member("u-mona", "Mona", "owner"), member("u-tom", "Tom"), member("u-ines", "Inès", "guest")]);
});
afterEach(() => {
  Element.prototype.getBoundingClientRect = realRect;
  vi.useRealTimers();
  forgetPeople();
  publishPages(null, null);
});

const publish = (nodes: PageNode[]) => act(async () => publishPages("space", nodes));

type Context = Partial<MentionContext>;

/** L'éditeur de la page `HERE`, avec ses menus de mention, le curseur à la fin du bloc `y`. */
async function mount(context: Context = {}, blocks: Seed = PAGE): Promise<Mounted> {
  const mentions: MentionContext = {
    spaceId: "space",
    role: "member",
    pageId: HERE,
    roster: () => ROSTER,
    canCreate: true,
    ...context,
  };
  const m = await mountEditor({
    blocks,
    view: (editor) => <EditorView editor={editor} editable mentions={mentions} />,
  });
  act(() => {
    m.editor.focus();
    cursor(m, "y", "Voir ".length);
  });
  return m;
}

/** Tape `text` au curseur, caractère par caractère, comme un clavier (ProseMirror, `handleTextInput`). */
async function type(m: Mounted, text: string): Promise<void> {
  const view = m.editor.prosemirrorView;
  for (const char of text) {
    await act(async () => {
      const { from, to } = view.state.selection;
      const insert = () => view.state.tr.insertText(char, from, to);
      const handled = view.someProp("handleTextInput", (handle) => handle(view, from, to, char, insert));
      if (!handled) view.dispatch(insert());
    });
    await flush();
  }
}

const menu = () => document.querySelector<HTMLElement>('[role="listbox"]');

/** Ce que le menu propose : ses groupes, et les lignes de chacun. */
function offered(): Record<string, string[]> {
  const groups: Record<string, string[]> = {};
  for (const group of menu()?.querySelectorAll('[role="group"]') ?? []) {
    const name = document.getElementById(group.getAttribute("aria-labelledby")!)!.textContent!;
    groups[name] = [...group.querySelectorAll('[role="option"]')].map((o) => o.textContent ?? "");
  }
  return groups;
}

const active = () => menu()?.querySelector('[role="option"][aria-selected="true"]')?.textContent ?? null;

const option = (text: string) =>
  [...(menu()?.querySelectorAll<HTMLElement>('[role="option"]') ?? [])].find((o) => o.textContent === text) ?? null;

/** Le menu se ferme en fondu : il quitte le document un instant après. */
const closed = (m: Mounted) => until(m.host, () => menu() === null, "le menu fermé");

/** Une touche dans le texte de l'éditeur. Entrée et Échap ferment le menu : on attend qu'il soit parti. */
async function key(m: Mounted, name: string): Promise<KeyboardEvent> {
  let event!: KeyboardEvent;
  await act(async () => {
    event = press(m.editor.prosemirrorView.dom, name);
  });
  await flush();
  if (name === "Enter" || name === "Escape") await closed(m);
  return event;
}

/** Un clic de souris sur `target` (`detail` : 1, ou 2 pour le second d'un double clic). */
async function click(target: Element, detail = 1): Promise<void> {
  await act(async () => {
    target.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, detail }));
    target.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail }));
  });
  await flush();
}

/** Le texte du bloc `y`, mentions comprises (`⟦@libellé⟧`). */
const textOfY = (m: Mounted) => shownInline(m).find((b) => b.id === "y")!.text;

/** Les mentions du document de `m`, dans l'ordre. */
function mentions(m: Mounted): Record<string, unknown>[] {
  const found: Record<string, unknown>[] = [];
  m.editor.prosemirrorState.doc.descendants((node) => {
    if (node.type.name === "mention") found.push({ ...node.attrs });
  });
  return found;
}

const tenPages = () => Array.from({ length: 10 }, (_, i) => page(pageId(i + 1), { title: `Page ${String(i + 1).padStart(2, "0")}` }));

test("at_menu_offers_pages_people_and_a_date", async () => {
  await publish(tenPages());
  members.mockResolvedValue(Array.from({ length: 10 }, (_, i) => member(`u-${i}`, `Personne ${i}`)));
  const m = await mount();
  // Aucune lecture des personnes avant l'ouverture du menu.
  expect(members).not.toHaveBeenCalled();
  expect(menu()).toBeNull();

  await type(m, "@");
  expect(menu()?.getAttribute("aria-label")).toBe("Mentionner");
  expect(members.mock.calls).toEqual([["space"]]);
  // Trois groupes, huit lignes au plus chacun.
  expect(offered()).toEqual({
    Pages: ["Page 01", "Page 02", "Page 03", "Page 04", "Page 05", "Page 06", "Page 07", "Page 08"],
    Personnes: Array.from({ length: 8 }, (_, i) => `Personne ${i}`),
    Dates: ["16 nov. 2026Aujourd'hui", "17 nov. 2026Demain"],
  });
  // Le texte de l'éditeur nomme la liste et sa ligne active : le focus n'en sort pas.
  const text = m.editor.prosemirrorView.dom;
  expect(text.getAttribute("aria-controls")).toBe(menu()!.id);
  expect(document.getElementById(text.getAttribute("aria-activedescendant")!)?.textContent).toBe("Page 01");

  // Flèches et Entrée : la deuxième page est posée, suivie d'une espace, à la place de « @ ».
  expect(active()).toBe("Page 01");
  await key(m, "ArrowDown");
  expect(active()).toBe("Page 02");
  await key(m, "ArrowDown");
  await key(m, "ArrowUp");
  expect(active()).toBe("Page 02");
  const enter = await key(m, "Enter");
  expect(enter.defaultPrevented).toBe(true);
  expect(menu()).toBeNull();
  expect(textOfY(m)).toBe("Voir ⟦@Page 02⟧ ");
  expect(mentions(m)).toEqual([{ kind: "page", id: pageId(2), label: "Page 02" }]);
  // La frappe reprend après l'espace.
  await type(m, "puis");
  expect(textOfY(m)).toBe("Voir ⟦@Page 02⟧ puis");
  expect(m.errors).toEqual([]);
});

test("the_at_menu_follows_what_is_typed_and_poses_a_person_or_a_date", async () => {
  await publish([page(pageId(1), { title: "Tomates" }), page(pageId(2), { title: "Plan" })]);
  const m = await mount();
  await type(m, "@to");
  // Sans accents ni casse, dans les titres comme dans les noms ; aucune date ne se dit « to ».
  expect(offered()).toEqual({ Pages: ["Tomates"], Personnes: ["Tom"] });
  await key(m, "ArrowDown");
  await key(m, "Enter");
  expect(textOfY(m)).toBe("Voir ⟦@Tom⟧ ");
  expect(mentions(m)).toEqual([{ kind: "user", id: "u-tom", label: "Tom" }]);

  await type(m, "@ine");
  expect(offered()).toEqual({ Personnes: ["Inès"] });
  await key(m, "Escape");
  await type(m, " @demain");
  expect(offered()).toEqual({ Dates: ["17 nov. 2026Demain"] });
  await key(m, "Enter");
  expect(mentions(m)[1]).toEqual({ kind: "date", id: "2026-11-17", label: "17 nov. 2026" });

  // Une date en chiffres garde une ligne tout du long, et se pose.
  for (const char of "@01/12/2027") {
    await type(m, char);
    if (char !== "@" && char !== "0") expect(menu(), char).not.toBeNull();
  }
  expect(offered()).toEqual({ Dates: ["1 déc. 2027"] });
  await key(m, "Enter");
  expect(mentions(m)[2]).toEqual({ kind: "date", id: "2027-12-01", label: "1 déc. 2027" });
  // Les membres n'ont été lus qu'une fois pour toutes ces frappes.
  expect(members).toHaveBeenCalledTimes(1);
});

test("escape_closes_the_menu_and_keeps_what_was_typed", async () => {
  await publish(tenPages());
  const m = await mount();
  await type(m, "@Pa");
  expect(menu()).not.toBeNull();
  const before = m.writes.length;
  await key(m, "Escape");
  expect(menu()).toBeNull();
  expect(textOfY(m)).toBe("Voir @Pa");
  expect(m.writes.length).toBe(before);
  // La frappe continue, sans rouvrir le menu.
  await type(m, "ge");
  expect(textOfY(m)).toBe("Voir @Page");
  expect(menu()).toBeNull();
  // Pareil pour `[[`.
  await type(m, " [[Pa");
  expect(menu()).not.toBeNull();
  await key(m, "Escape");
  expect(menu()).toBeNull();
  expect(textOfY(m)).toBe("Voir @Page [[Pa");
});

test("guest_is_offered_only_who_is_on_the_page", async () => {
  // L'arbre d'un invité ne porte que ses pages ; la route des membres lui est fermée.
  await publish([page(pageId(1), { title: "La page de l'invitée" })]);
  const m = await mount({ role: "guest" });
  await type(m, "@");
  expect(offered()).toEqual({
    Pages: ["La page de l'invitée"],
    Personnes: ["Mona", "Inès"],
    Dates: ["16 nov. 2026Aujourd'hui", "17 nov. 2026Demain"],
  });
  expect(members).not.toHaveBeenCalled();
  // Tom, membre de l'espace, n'a pas la page ouverte : il n'est pas proposé.
  await type(m, "tom");
  expect(offered()).toEqual({});
  // Témoin : un membre, sur la même page, se le voit proposer.
  await key(m, "Escape");
  const witness = await mount({ role: "member" });
  await type(witness, "@tom");
  expect(offered()).toEqual({ Personnes: ["Tom"] });
});

test("double_bracket_offers_pages_and_creating_one", async () => {
  await publish([page(pageId(1), { title: "Compte" }), page(pageId(2), { title: "Plan" })]);
  const m = await mount();
  await type(m, "[");
  expect(menu()).toBeNull();
  await type(m, "[");
  expect(menu()?.getAttribute("aria-label")).toBe("Lier une page");
  // Requête vide : les pages, sans « Créer la page ». Ni personnes ni dates.
  expect(offered()).toEqual({ Pages: ["Compte", "Plan"] });
  expect(members).not.toHaveBeenCalled();
  await type(m, "Compte");
  expect(offered()).toEqual({ Pages: ["Compte", "Créer la page « Compte »"] });
  // Un titre qu'aucune page ne porte : « Créer la page », seule, et en dernier.
  await type(m, " rendu");
  expect(offered()).toEqual({ Pages: ["Créer la page « Compte rendu »"] });
  // Choisir une page pose sa mention, à la place de `[[` et de ce qu'on avait tapé.
  await key(m, "Escape");
  await type(m, " [[pl");
  await key(m, "Enter");
  expect(textOfY(m)).toBe("Voir [[Compte rendu ⟦@Plan⟧ ");
  expect(mentions(m)).toEqual([{ kind: "page", id: pageId(2), label: "Plan" }]);
  expect(creates).not.toHaveBeenCalled();
});

test("creating_is_not_offered_without_the_right_to_edit", async () => {
  await publish([page(pageId(1), { title: "Compte" })]);
  const m = await mount({ canCreate: false });
  await type(m, "[[Compte");
  expect(offered()).toEqual({ Pages: ["Compte"] });
  // Témoin : avec le droit, la même frappe l'offre.
  await key(m, "Escape");
  const witness = await mount({ canCreate: true });
  await type(witness, "[[Compte");
  expect(offered()).toEqual({ Pages: ["Compte", "Créer la page « Compte »"] });
});

test("creating_from_double_bracket_creates_a_subpage_once", async () => {
  await publish([]);
  const m = await mount();
  const answer = deferred<PageNode>();
  creates.mockReturnValue(answer.promise);
  await type(m, "[[ Compte rendu ");
  const create = option("Créer la page « Compte rendu »")!;
  // Le second clic d'un double clic ne crée rien.
  await click(create, 2);
  expect(creates).not.toHaveBeenCalled();
  expect(menu()).not.toBeNull();
  await click(create, 1);
  await closed(m);
  // Une sous-page de la page ouverte, au titre tapé ; le menu fermé, `[[` et le titre retirés du texte.
  expect(creates.mock.calls).toEqual([[{ parentId: HERE, title: "Compte rendu" }]]);
  expect(menu()).toBeNull();
  expect(textOfY(m)).toBe("Voir ");
  // En attendant le serveur : une autre demande ne crée pas une seconde page, et la frappe continue.
  await type(m, "[[Autre");
  await key(m, "Enter");
  expect(creates).toHaveBeenCalledTimes(1);
  await type(m, "la suite");
  expect(textOfY(m)).toBe("Voir la suite");
  // La page créée : sa mention va là où l'on avait tapé `[[`, avec le titre que le serveur a gardé.
  await act(async () => answer.resolve(page(pageId(9), { title: "Compte rendu", parentId: HERE })));
  await flush();
  expect(textOfY(m)).toBe("Voir ⟦@Compte rendu⟧ la suite");
  expect(mentions(m)).toEqual([{ kind: "page", id: pageId(9), label: "Compte rendu" }]);
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(m.errors).toEqual([]);
});

test("a_refused_creation_is_said_as_the_server_says_it_and_nothing_is_inserted", async () => {
  await publish([]);
  const onRefused = vi.fn();
  const m = await mount({ onRefused });
  const refusal = "Il faut « Peut modifier » sur la page pour y créer une sous-page.";
  creates.mockRejectedValue(new ApiError(403, "forbidden", refusal));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  const alert = document.querySelector('[role="alert"]');
  expect(alert?.textContent).toBe(createRefused("Compte rendu", refusal));
  expect(mentions(m)).toEqual([]);
  expect(textOfY(m)).toBe("Voir ");
  // La page est à relire : un droit a changé.
  expect(onRefused).toHaveBeenCalledTimes(1);
  // Le focus est resté dans le texte ; Échap ferme le message.
  expect(m.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
  await key(m, "Escape");
  expect(document.querySelector('[role="alert"]')).toBeNull();

  // Une session perdue : la connexion est demandée ailleurs, rien n'est dit ici, la page n'est pas relue.
  creates.mockRejectedValue(unauthenticated());
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(onRefused).toHaveBeenCalledTimes(1);
  // Après un refus, on peut redemander.
  creates.mockResolvedValue(page(pageId(9), { title: "Compte rendu" }));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  expect(creates).toHaveBeenCalledTimes(3);
  expect(textOfY(m)).toBe("Voir ⟦@Compte rendu⟧ ");
});

test("a_page_created_whose_place_is_gone_is_said_and_written_nowhere_else", async () => {
  await publish([]);
  const m = await mount();
  const answer = deferred<PageNode>();
  creates.mockReturnValue(answer.promise);
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  // Quelqu'un supprime le bloc pendant que le serveur crée la page.
  act(() => m.editor.removeBlocks(["y"]));
  const before = m.writes.length;
  await act(async () => answer.resolve(page(pageId(9), { title: "Compte rendu" })));
  await flush();
  expect(mentions(m)).toEqual([]);
  expect(m.writes.length).toBe(before);
  expect(document.querySelector('[role="alert"]')?.textContent).toBe(createdNotPlaced("Compte rendu"));
  // Le bouton du message le ferme, et rend le focus au texte : il ne tombe pas sur `body`.
  const close = document.querySelector<HTMLElement>('[role="alert"] button')!;
  close.focus();
  expect(document.activeElement).toBe(close);
  await click(close);
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(m.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
});

test("a_mention_takes_the_title_of_the_moment", async () => {
  await publish([page(pageId(1), { title: "Plan" })]);
  const m = await mount();
  await type(m, "@pl");
  expect(offered()).toEqual({ Pages: ["Plan"] });
  // Renommée pendant que le menu est ouvert : la liste ne bouge pas, la mention prend le titre à jour.
  await publish([page(pageId(1), { title: "Plan 2027" })]);
  expect(offered()).toEqual({ Pages: ["Plan"] });
  await key(m, "Enter");
  expect(mentions(m)).toEqual([{ kind: "page", id: pageId(1), label: "Plan 2027" }]);
  // Partie de l'arbre entre-temps : la mention garde le titre qu'on a choisi.
  await type(m, "@pl");
  await publish([]);
  await key(m, "Enter");
  expect(mentions(m)[1]).toEqual({ kind: "page", id: pageId(1), label: "Plan 2027" });
});

test("the_menus_stay_closed_in_a_code_block", async () => {
  await publish(tenPages());
  const m = await mount();
  act(() => cursor(m, "c", "code ".length));
  await type(m, "@");
  expect(menu()).toBeNull();
  await type(m, " [[");
  expect(menu()).toBeNull();
  expect(shownInline(m).find((b) => b.id === "c")!.text).toBe("code @ [[");
  // Témoin : hors du bloc de code, la même frappe ouvre le menu.
  act(() => cursor(m, "y", "Voir ".length));
  await type(m, "@");
  expect(menu()).not.toBeNull();
});

test("a_menu_line_is_text_isolated_and_a_click_keeps_the_focus_in_the_text", async () => {
  const tag = '<img src="x" onerror="alert(1)">';
  await publish([page(pageId(1), { title: tag })]);
  members.mockResolvedValue([member("u-x", "<b>Ève</b>")]);
  const m = await mount();
  await type(m, "@<");
  expect(offered()).toEqual({ Pages: [tag], Personnes: ["<b>Ève</b>"] });
  expect(menu()!.querySelector("img, b")).toBeNull();
  for (const title of menu()!.querySelectorAll(".mm-mention-option-title")) expect(title.getAttribute("dir")).toBe("auto");
  // Un clic choisit sans prendre le focus au texte : la fenêtre de BlockNote retient le `mousedown`
  // (une garde : à revoir rouge si une montée de version le retire).
  const line = option("<b>Ève</b>")!;
  const down = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
  line.dispatchEvent(down);
  expect(down.defaultPrevented).toBe(true);
  await click(line);
  await closed(m);
  expect(mentions(m)).toEqual([{ kind: "user", id: "u-x", label: "<b>Ève</b>" }]);
  expect(m.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
});

test("what_another_types_while_the_menu_is_open_is_kept", async () => {
  await publish([page(pageId(1), { title: "Plan" })]);
  const m = await mount();
  const server = new Y.Doc();
  Y.applyUpdate(server, Y.encodeStateAsUpdate(m.doc));
  const tom = await peer("Tom", 9, Y.encodeStateAsUpdate(m.doc));
  await type(m, "@pl");
  // Tom tape dans le bloc voisin, puis au début du bloc où le menu est ouvert.
  typeAtEnd(tom, "x", " de Tom");
  typeAt(tom, "y", 0, "Tom : ");
  await act(async () => exchange(server, [tom, m]));
  await flush();
  expect(offered()).toEqual({ Pages: ["Plan"] });
  await key(m, "Enter");
  expect(shownInline(m).find((b) => b.id === "x")!.text).toBe("Bloc X de Tom");
  expect(textOfY(m)).toBe("Tom : Voir ⟦@Plan⟧ ");
  await act(async () => exchange(server, [m, tom]));
  expect(shownInline(tom)).toEqual(shownInline(m));
});

test("a_slash_a_colon_or_an_at_sign_typed_in_a_mention_menu_belongs_to_it", async () => {
  await publish([page(pageId(1), { title: "Entrées/sorties : le plan" })]);
  const m = await mount();
  // Dans `[[`, « / », « : » et « @ » sont des caractères du titre : ni menu « / », ni emoji, ni `@`.
  await type(m, "[[Entrées/sorties : le");
  expect(menu()?.getAttribute("aria-label")).toBe("Lier une page");
  expect(offered()).toEqual({ Pages: ["Entrées/sorties : le plan", "Créer la page « Entrées/sorties : le »"] });
  await type(m, " @");
  expect(menu()?.getAttribute("aria-label")).toBe("Lier une page");
  expect(offered()).toEqual({ Pages: ["Créer la page « Entrées/sorties : le @ »"] });
  await key(m, "Escape");
  // Témoin : le menu fermé, « / » ouvre le sien.
  await type(m, " /");
  expect(menu()).not.toBeNull();
  expect(menu()?.getAttribute("aria-label")).not.toBe("Lier une page");
});
