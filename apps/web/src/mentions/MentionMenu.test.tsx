// Les menus `@` et `[[` (Task C4, R251) : ce qu'ils proposent, à qui, et ce qu'ils posent dans le
// document. Aucun ne propose une page que le compte ne voit pas : les pages viennent de l'arbre de
// l'espace affiché, déjà filtré par les droits (`pageDirectory.ts`).
import type { Member, PageNode } from "@mementum/protocol";
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { ApiError, createPage, getMembers, getTree } from "../api";
import type { RosterUser } from "../editor/collab";
import { focusEditor } from "../editor/blocks/BlockWindowHost";
import { EditorView } from "../editor/Editor";
import { publishPages } from "../space/pageDirectory";
import { cursor, exchange, peer, shownInline, typeAt, typeAtEnd, type Seed } from "../test/editors";
import { deferred, until } from "../test/hooks";
import { page, unauthenticated } from "../test/pages";
import { flush, mountEditor, press, type Mounted } from "../test/richEditor";
import { createdNotPlaced, createLostAndSeen, createMaybeMade, createNotMade, createRefused, createWaits, type MentionContext } from "./MentionMenu";
import { clearNotices, MentionNotices, PAGE_LEFT } from "./notice";
import { forgetPeople } from "./people";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMembers: vi.fn(),
  createPage: vi.fn(),
  getTree: vi.fn(),
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
  clearNotices();
});

const publish = (nodes: PageNode[]) => act(async () => publishPages("space", nodes));

type Context = Partial<MentionContext>;

const contextOf = (context: Context = {}): MentionContext => ({
  spaceId: "space",
  role: "member",
  pageId: HERE,
  roster: () => ROSTER,
  canCreate: true,
  ...context,
});

/** La page autour de l'éditeur : ses messages (comme `PageView` les rend), puis l'éditeur et ses menus. */
const pageOf = (editor: Mounted["editor"], context: Context = {}) => (
  <>
    <MentionNotices focusBack={() => focusEditor(editor)} />
    <EditorView editor={editor} editable mentions={contextOf(context)} />
  </>
);

/** L'éditeur de la page `HERE`, avec ses menus de mention, le curseur à la fin du bloc `y`. */
async function mount(context: Context = {}, blocks: Seed = PAGE): Promise<Mounted> {
  const m = await mountEditor({ blocks, view: (editor) => pageOf(editor, context) });
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
  // En attendant le serveur, la frappe continue.
  await type(m, "la suite");
  expect(textOfY(m)).toBe("Voir la suite");
  // La page créée : sa mention va là où l'on avait tapé `[[`, avec le titre que le serveur a gardé.
  await act(async () => answer.resolve(page(pageId(9), { title: "Compte rendu", parentId: HERE })));
  await flush();
  expect(textOfY(m)).toBe("Voir ⟦@Compte rendu⟧ la suite");
  expect(mentions(m)).toEqual([{ kind: "page", id: pageId(9), label: "Compte rendu" }]);
  expect(creates).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(m.errors).toEqual([]);
});

/** Les messages de la page (ceux de son premier hôte : un test qui monte deux éditeurs en a deux). */
const alerts = () => [...(document.querySelector('[role="alert"]')?.querySelectorAll("p") ?? [])].map((p) => p.textContent);

test("a_second_page_asked_while_the_first_is_being_created_is_created_too", async () => {
  await publish([]);
  const m = await mount();
  const first = deferred<PageNode>();
  const second = deferred<PageNode>();
  creates.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  await type(m, "[[Un");
  await key(m, "Enter");
  // Réseau lent : la première n'a pas répondu, on en demande une autre, d'un autre titre.
  await type(m, "et [[Deux");
  await key(m, "Enter");
  expect(creates.mock.calls).toEqual([[{ parentId: HERE, title: "Un" }], [{ parentId: HERE, title: "Deux" }]]);
  await type(m, "puis");
  // La seconde répond la première : chacune va à sa place.
  await act(async () => second.resolve(page(pageId(8), { title: "Deux" })));
  await flush();
  expect(textOfY(m)).toBe("Voir et ⟦@Deux⟧ puis");
  await act(async () => first.resolve(page(pageId(7), { title: "Un" })));
  await flush();
  expect(textOfY(m)).toBe("Voir ⟦@Un⟧ et ⟦@Deux⟧ puis");
  expect(alerts()).toEqual([]);
  // Deux refus à la suite : les deux titres sont dits.
  creates.mockReset();
  creates.mockRejectedValue(new ApiError(403, "forbidden", "Non."));
  await type(m, " [[Trois");
  await key(m, "Enter");
  await type(m, "[[Quatre");
  await key(m, "Enter");
  await flush();
  expect(alerts()).toEqual([createRefused("Trois", "Non."), createRefused("Quatre", "Non.")]);
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
  // Le focus est resté dans le texte ; un Échap tapé là ne ferme pas le message (R632), son bouton si.
  expect(m.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
  await act(async () => {
    press(m.editor.prosemirrorView.dom, "Escape");
  });
  expect(document.querySelector('[role="alert"]')?.textContent).toBe(createRefused("Compte rendu", refusal));
  await click(document.querySelector('[role="alert"] button')!);
  expect(document.querySelector('[role="alert"]')).toBeNull();

  // Après un refus, on peut redemander.
  creates.mockResolvedValue(page(pageId(9), { title: "Compte rendu" }));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  expect(creates).toHaveBeenCalledTimes(2);
  expect(textOfY(m)).toBe("Voir ⟦@Compte rendu⟧ ");
});

test("a_creation_refused_for_a_lost_session_names_the_title_and_is_asked_again_once_the_account_is_back", async () => {
  await publish([]);
  const back = deferred<void>();
  const onRefused = vi.fn();
  const m = await mount({ afterLogin: () => back.promise, onRefused });
  creates.mockRejectedValueOnce(unauthenticated());
  creates.mockResolvedValue(page(pageId(9), { title: "Compte rendu" }));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  // Le `401` n'a rien créé. Le titre tapé a quitté le texte : un message le nomme, sous la fenêtre de connexion.
  expect(textOfY(m)).toBe("Voir ");
  expect(alerts()).toEqual([createWaits("Compte rendu")]);
  expect(creates).toHaveBeenCalledTimes(1);
  expect(onRefused).not.toHaveBeenCalled();
  await type(m, "la suite");
  // Le compte revenu : la création est redemandée, la mention posée à sa place, le message retiré.
  await act(async () => back.resolve());
  await flush();
  expect(creates.mock.calls).toEqual([[{ parentId: HERE, title: "Compte rendu" }], [{ parentId: HERE, title: "Compte rendu" }]]);
  expect(textOfY(m)).toBe("Voir ⟦@Compte rendu⟧ la suite");
  expect(alerts()).toEqual([]);
});

test("a_creation_that_fails_again_or_whose_account_never_returns_keeps_a_message_naming_the_title", async () => {
  await publish([]);
  const back = deferred<void>();
  const m = await mount({ afterLogin: () => back.promise });
  // Refusée encore au retour du compte : le refus du serveur remplace l'attente.
  creates.mockRejectedValueOnce(unauthenticated());
  creates.mockRejectedValueOnce(new ApiError(403, "forbidden", "Tu ne peux plus modifier cette page."));
  await type(m, "[[Un");
  await key(m, "Enter");
  await flush();
  expect(alerts()).toEqual([createWaits("Un")]);
  await act(async () => back.resolve());
  await flush();
  expect(alerts()).toEqual([createRefused("Un", "Tu ne peux plus modifier cette page.")]);
  expect(mentions(m)).toEqual([]);
  // Le compte ne revient pas (ou rien ne dit qu'il revient) : le message reste, avec le titre.
  act(() => clearNotices());
  const alone = await mount({ afterLogin: undefined });
  creates.mockRejectedValueOnce(unauthenticated());
  await type(alone, "[[Deux");
  await key(alone, "Enter");
  await flush();
  expect(alerts()).toEqual([createWaits("Deux")]);
  expect(creates).toHaveBeenCalledTimes(3);
});

test("a_session_lost_again_at_the_second_try_is_said_without_reading_the_page_again", async () => {
  await publish([]);
  const back = deferred<void>();
  const onRefused = vi.fn();
  const m = await mount({ afterLogin: () => back.promise, onRefused });
  creates.mockRejectedValue(unauthenticated());
  await type(m, "[[Un");
  await key(m, "Enter");
  await flush();
  await act(async () => back.resolve());
  await flush();
  // Une seule nouvelle demande ; le message nomme toujours le titre ; ce n'est pas un droit qui a
  // changé : la page n'est pas relue.
  expect(creates).toHaveBeenCalledTimes(2);
  expect(alerts()).toEqual([createRefused("Un", "session absente ou échue : se connecter")]);
  expect(onRefused).not.toHaveBeenCalled();
  // Témoin : un refus du serveur, lui, fait relire la page.
  creates.mockRejectedValue(new ApiError(403, "forbidden", "Non."));
  await type(m, "[[Deux");
  await key(m, "Enter");
  await flush();
  expect(onRefused).toHaveBeenCalledTimes(1);
});

test("escape_on_the_message_button_closes_it_and_gives_the_focus_back_to_the_text", async () => {
  await publish([]);
  const m = await mount();
  creates.mockRejectedValue(new ApiError(403, "forbidden", "Tu ne peux pas."));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  const close = document.querySelector<HTMLElement>('[role="alert"] button')!;
  close.focus();
  expect(document.activeElement).toBe(close);
  await act(async () => {
    press(close, "Escape");
  });
  await flush();
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(m.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
});

test("escape_typed_for_something_else_leaves_the_message_that_names_the_title", async () => {
  await publish([page(pageId(1), { title: "Plan" })]);
  const m = await mount();
  creates.mockRejectedValue(new ApiError(403, "forbidden", "Non."));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  expect(alerts()).toEqual([createRefused("Compte rendu", "Non.")]);
  // Échap pour fermer un menu `@` : le menu se ferme, le message reste.
  await type(m, "@pl");
  await key(m, "Escape");
  expect(menu()).toBeNull();
  expect(alerts()).toEqual([createRefused("Compte rendu", "Non.")]);
  // Échap dans un autre champ (le titre de la page) : de même, et le focus n'a pas bougé.
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  elsewhere.focus();
  await act(async () => {
    press(elsewhere, "Escape");
  });
  expect(alerts()).toEqual([createRefused("Compte rendu", "Non.")]);
  expect(document.activeElement).toBe(elsewhere);
  elsewhere.remove();
  // Témoin : le focus dans le message, Échap le ferme.
  const close = document.querySelector<HTMLElement>('[role="alert"] button')!;
  close.focus();
  await act(async () => {
    press(close, "Escape");
  });
  expect(alerts()).toEqual([]);
});

test("a_waiting_message_closed_does_not_silence_what_comes_of_the_second_try", async () => {
  await publish([]);
  // Refusée au second essai, le message d'attente fermé entre-temps : le refus est dit quand même.
  const back = deferred<void>();
  const m = await mount({ afterLogin: () => back.promise });
  creates.mockRejectedValueOnce(unauthenticated());
  creates.mockRejectedValueOnce(new ApiError(403, "forbidden", "Tu ne peux plus modifier cette page."));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  expect(alerts()).toEqual([createWaits("Compte rendu")]);
  await click(document.querySelector('[role="alert"] button')!);
  expect(alerts()).toEqual([]);
  await act(async () => back.resolve());
  await flush();
  expect(alerts()).toEqual([createRefused("Compte rendu", "Tu ne peux plus modifier cette page.")]);
  // Créée au second essai, sa place partie : dit aussi.
  act(() => clearNotices());
  const again = deferred<void>();
  await act(async () => m.root.render(pageOf(m.editor, { afterLogin: () => again.promise })));
  creates.mockRejectedValueOnce(unauthenticated());
  creates.mockResolvedValueOnce(page(pageId(9), { title: "Notes" }));
  await type(m, "[[Notes");
  await key(m, "Enter");
  await flush();
  expect(alerts()).toEqual([createWaits("Notes")]);
  await click(document.querySelector('[role="alert"] button')!);
  act(() => m.editor.removeBlocks(["y"]));
  await act(async () => again.resolve());
  await flush();
  expect(alerts()).toEqual([createdNotPlaced("Notes")]);
  expect(creates).toHaveBeenCalledTimes(4);
});

test("a_page_left_while_its_creation_waits_for_the_account_says_it_was_not_created", async () => {
  await publish([]);
  // L'attente du compte est celle de la page : quittée, elle est rompue (`PAGE_LEFT`).
  const wait = deferred<void>();
  const m = await mount({ afterLogin: () => wait.promise });
  creates.mockRejectedValueOnce(unauthenticated());
  creates.mockResolvedValue(page(pageId(9), { title: "Compte rendu" }));
  await type(m, "[[Compte rendu");
  await key(m, "Enter");
  await flush();
  expect(alerts()).toEqual([createWaits("Compte rendu")]);
  await act(async () => wait.reject(PAGE_LEFT));
  await flush();
  // Rien n'est créé, et le message ne promet plus rien.
  expect(creates).toHaveBeenCalledTimes(1);
  expect(alerts()).toEqual([createNotMade("Compte rendu")]);
  expect(mentions(m)).toEqual([]);
});

test("a_block_replaced_by_another_at_the_same_place_during_the_creation_gets_no_mention", async () => {
  await publish([]);
  const m = await mount();
  const answer = deferred<PageNode>();
  creates.mockReturnValue(answer.promise);
  await type(m, "[[Notes");
  await key(m, "Enter");
  // Le bloc part, un autre prend sa place : la place suivie y retombe, au milieu d'un mot.
  act(() => void m.editor.replaceBlocks(["y"], [{ id: "w", type: "paragraph", content: "Autre texte ici" }]));
  await act(async () => answer.resolve(page(pageId(9), { title: "Notes" })));
  await flush();
  expect(shownInline(m).find((b) => b.id === "w")!.text).toBe("Autre texte ici");
  expect(mentions(m)).toEqual([]);
  expect(alerts()).toEqual([createdNotPlaced("Notes")]);
});

test("the_at_list_is_not_read_again_under_the_active_line_when_the_context_changes", async () => {
  await publish([page(pageId(1), { title: "Plan A" }), page(pageId(2), { title: "Plan B" })]);
  const m = await mount();
  await type(m, "@Plan");
  await key(m, "ArrowDown");
  expect(active()).toBe("Plan B");
  await publish([page(pageId(3), { title: "Plan 0" }), page(pageId(1), { title: "Plan A" }), page(pageId(2), { title: "Plan B" })]);
  await act(async () => m.root.render(pageOf(m.editor, { role: "admin" })));
  await flush();
  expect(active()).toBe("Plan B");
  await key(m, "Enter");
  expect(mentions(m).map((x) => x.label)).toEqual(["Plan B"]);
});

test("a_page_created_once_its_editor_is_gone_is_said_by_the_page", async () => {
  await publish([]);
  const m = await mount();
  const answer = deferred<PageNode>();
  creates.mockReturnValue(answer.promise);
  await type(m, "[[Orpheline");
  await key(m, "Enter");
  // La session est recréée (ou une autre page ouverte) : l'éditeur et ses menus partent, la page reste.
  await act(async () => m.root.render(<MentionNotices focusBack={() => {}} />));
  await act(async () => answer.resolve(page(pageId(9), { title: "Orpheline" })));
  await flush();
  expect(alerts()).toEqual([createdNotPlaced("Orpheline")]);
  expect(m.errors).toEqual([]);
});

for (const kind of ["heading", "quote", "codeBlock"] as const) {
  const outcome = kind === "codeBlock" ? "gets_a_message" : "keeps_its_mention";
  test(`a_block_turned_into_${kind}_during_the_creation_${outcome}`, async () => {
    await publish([]);
    const m = await mount();
    const answer = deferred<PageNode>();
    creates.mockReturnValue(answer.promise);
    await type(m, "[[Notes");
    await key(m, "Enter");
    // Changer le type d'un bloc recrée son élément de contenu : la place tenue n'y est plus.
    act(() => void m.editor.updateBlock("y", { type: kind }));
    await act(async () => answer.resolve(page(pageId(9), { title: "Notes" })));
    await flush();
    if (kind === "codeBlock") {
      // Un bloc de code ne porte pas de mention : rien n'est posé, et c'est dit.
      expect(mentions(m)).toEqual([]);
      expect(alerts()).toEqual([createdNotPlaced("Notes")]);
    } else {
      // Le bloc est retrouvé par son id : la mention va à la fin de son texte.
      expect(textOfY(m)).toBe("Voir ⟦@Notes⟧ ");
      expect(shownInline(m).find((b) => b.id === "y")!.type).toBe(kind);
      expect(alerts()).toEqual([]);
    }
    expect(m.errors).toEqual([]);
  });
}

test("the_list_is_not_read_again_under_the_active_line_when_the_context_changes", async () => {
  await publish([page(pageId(1), { title: "Plan A" }), page(pageId(2), { title: "Plan B" })]);
  const m = await mount();
  await type(m, "[[Plan");
  await key(m, "ArrowDown");
  expect(active()).toBe("Plan B");
  // Une page arrive dans l'arbre, puis le compte est relu avec un autre rôle : `PageView` donne aux
  // menus un contexte neuf. La liste ne bouge pas, et Entrée pose la page choisie.
  await publish([page(pageId(3), { title: "Plan 0" }), page(pageId(1), { title: "Plan A" }), page(pageId(2), { title: "Plan B" })]);
  await act(async () => m.root.render(pageOf(m.editor, { role: "admin", canCreate: false })));
  await flush();
  expect(active()).toBe("Plan B");
  await key(m, "Enter");
  expect(mentions(m).map((x) => x.label)).toEqual(["Plan B"]);
  // Le contexte neuf sert à la frappe suivante : sans le droit de créer, « Créer la page » n'est plus offert.
  await type(m, "[[Plan Z");
  expect(offered()).toEqual({});
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

test("a_creation_whose_answer_is_lost_reads_the_tree_and_says_what_it_shows_without_inserting", async () => {
  // I1 (relecture finale de la partie B, R760) : la réponse perdue en route, le serveur a pu créer la
  // page. Ni « n'a pas pu être créée », ni mention posée au hasard : l'arbre relu dit si une page de
  // ce titre vient d'apparaître sous celle-ci.
  const trees = vi.mocked(getTree);
  const tree = (nodes: PageNode[]) => ({ spaceId: "space", lsn: 2, nodes });
  // Une page du même titre était déjà là, sous cette page : ce n'est pas elle qui vient d'apparaître.
  const old = page(pageId(1), { title: "Compte rendu", parentId: HERE });
  const made = page(pageId(2), { title: "Compte rendu", parentId: HERE });
  const elsewhere = page(pageId(3), { title: "Compte rendu", parentId: pageId(1) });
  const cases: [string, () => void, string][] = [
    ["la page a paru", () => trees.mockResolvedValueOnce(tree([old, made])), createLostAndSeen("Compte rendu")],
    ["rien de neuf", () => trees.mockResolvedValueOnce(tree([old])), createMaybeMade("Compte rendu")],
    ["ailleurs que sous la page", () => trees.mockResolvedValueOnce(tree([old, elsewhere])), createMaybeMade("Compte rendu")],
    ["arbre illisible", () => trees.mockRejectedValueOnce(new ApiError(0, null, "coupé")), createMaybeMade("Compte rendu")],
  ];
  for (const lost of [new ApiError(0, null, "coupé"), new ApiError(502, null, "mandataire")]) {
    for (const [what, answerTree, said] of cases) {
      await publish([old]);
      const onRefused = vi.fn();
      const m = await mount({ onRefused });
      trees.mockReset();
      answerTree();
      creates.mockReset();
      creates.mockRejectedValueOnce(lost);
      await type(m, "[[Compte rendu");
      await click(option("Créer la page « Compte rendu »")!);
      await flush();
      expect(alerts(), what).toEqual([said]);
      expect(trees, what).toHaveBeenCalledWith("space");
      // Rien n'est posé : on ne devine pas laquelle lier.
      expect(mentions(m), what).toEqual([]);
      expect(creates, what).toHaveBeenCalledTimes(1);
      expect(said).toContain("« Compte rendu »");
      expect(said).not.toContain("n'a pas pu");
      act(() => clearNotices());
      await closed(m);
    }
  }
});
