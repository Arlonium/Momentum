// Un éditeur de page rendu par React dans jsdom, avec ce que l'application pose autour de lui (le
// menu « / », la barre de mise en forme, la fenêtre de réglage d'un bloc : `EditorView`), lié à un
// `Y.Doc` sans fournisseur. Pour les tests des blocs riches (`editor/blocks/*.test.tsx`) : un bloc
// rendu par React (`createReactBlockSpec`) n'est vrai que dans `BlockNoteView`.
//
// L'éditeur rendu est aussi une personne du banc (`test/editors.ts`, `Peer`) : ce qu'il écrit
// s'échange avec un autre (`exchange`), qui peut être un éditeur du banc, sans React.
import { BlockNoteEditor } from "@blocknote/core";
import { withCollaboration } from "@blocknote/core/yjs";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach } from "vitest";
import * as Y from "yjs";
import { FRAGMENT } from "../editor/collab";
import type { MementumEditor } from "../editor/debug";
import { EDITOR_OPTIONS, EditorView } from "../editor/Editor";
import { REMOTE, seedState, type Peer, type Seed } from "./editors";
import { until } from "./hooks";

/**
 * Ce que jsdom n'a pas : `matchMedia` (Mantine lit le thème du système), et la géométrie d'un
 * intervalle de texte (la barre de mise en forme se place sur la sélection).
 */
function stubMatchMedia(): void {
  Range.prototype.getClientRects ??= () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
  window.matchMedia = ((media: string) => ({
    matches: false,
    media,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

export type Mounted = Peer & {
  host: HTMLElement;
  root: Root;
  /** Ce que React et l'éditeur ont écrit en erreur dans la console pendant le test. */
  errors: string[];
  /** Le texte affiché de l'éditeur, blocs et fenêtres comprises. */
  text(): string;
};

/** Les éditeurs rendus par le test en cours : démontés à sa fin, avec leur document. */
const rendered: Mounted[] = [];
let restoreConsole: (() => void) | null = null;
afterEach(async () => {
  for (const m of rendered.splice(0)) {
    await act(async () => m.root.unmount());
    m.host.remove();
    m.doc.destroy();
  }
  restoreConsole?.();
  restoreConsole = null;
});

type Options = {
  /** La page de départ, écrite comme par un autre client (ou l'état d'un document). */
  blocks?: Seed;
  base?: Uint8Array;
  /** Un document déjà rempli (un bloc écrit à la main, d'une forme que BlockNote n'écrirait pas). */
  doc?: Y.Doc;
  clientID?: number;
  editable?: boolean;
  /** La page de l'éditeur, comme `Editor` la donne à sa vue (`session.pageId`). */
  pageId?: string;
  /** De quoi envoyer un fichier, comme `Editor` le donne à l'éditeur d'une page qu'on modifie. */
  uploadFile?: (file: File, blockId?: string) => Promise<string | Record<string, unknown>>;
  /** Ce que React rend : par défaut, la vue de l'éditeur de la page. */
  view?: (editor: MementumEditor) => ReactElement;
};

/** L'éditeur de la page, rendu par React, sur une page de départ. */
export async function mountEditor({
  blocks,
  base,
  doc = new Y.Doc(),
  clientID = 7,
  editable = true,
  pageId,
  uploadFile,
  view = (editor) => <EditorView editor={editor} editable={editable} pageId={pageId} />,
}: Options = {}): Promise<Mounted> {
  stubMatchMedia();
  if (restoreConsole === null) {
    const original = console.error;
    const errors: string[] = [];
    console.error = (...args: unknown[]) => {
      errors.push(args.map((a) => (a instanceof Error ? `${a.message}` : String(a))).join(" "));
    };
    restoreConsole = () => {
      console.error = original;
    };
    consoleErrors = errors;
  }
  doc.clientID = clientID;
  const start = base ?? (blocks ? await seedState(blocks) : undefined);
  if (start) Y.applyUpdate(doc, start, REMOTE);
  const writes: Uint8Array[] = [];
  doc.on("update", (update: Uint8Array, origin: unknown) => {
    if (origin !== REMOTE) writes.push(update);
  });
  const editor = createEditor(doc, uploadFile);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(view(editor)));
  // Le premier rendu de y-prosemirror est différé.
  await act(async () => {
    await new Promise((done) => setTimeout(done, 0));
  });
  const mounted: Mounted = {
    name: "Mona",
    doc,
    editor,
    writes,
    host,
    root,
    errors: consoleErrors,
    text: () => host.textContent ?? "",
  };
  rendered.push(mounted);
  return mounted;
}

let consoleErrors: string[] = [];

/** Laisse React suivre ce qui vient de changer (un document reçu, un clic). */
export async function flush(): Promise<void> {
  await act(async () => {
    await new Promise((done) => setTimeout(done, 0));
  });
}

/** L'élément affiché du bloc `id` dans l'éditeur rendu `m`. */
export function blockOf(m: Mounted, id: string): HTMLElement {
  const element = m.host.querySelector<HTMLElement>(`[data-node-type="blockOuter"][data-id="${id}"]`);
  if (element === null) throw new Error(`le bloc ${id} n'est pas affiché`);
  return element;
}

/**
 * Les seuls attributs `style` d'un bloc qui ne viennent pas de nous : ceux que BlockNote pose
 * lui-même sur la vue d'un bloc et sur son contenu modifiable, fixes, qui ne disent rien du document.
 */
const BLOCKNOTE_STYLES = new Set(["white-space: normal;", "white-space: inherit;"]);

/**
 * Les attributs `style` d'un élément et de ses descendants, hors ceux de BlockNote : un bloc riche
 * n'en pose aucun (« jamais un attribut `style` » pour ce qui vient du document).
 */
export function foreignStyles(element: Element): string[] {
  return [element, ...element.querySelectorAll("[style]")]
    .filter((e) => e.hasAttribute("style") && !BLOCKNOTE_STYLES.has(e.getAttribute("style")!))
    .map((e) => `${e.tagName}.${[...e.classList].join(".")} ${e.getAttribute("style")}`);
}

/** Remplit un champ que React contrôle, comme une frappe (jsdom n'a pas de clavier). */
export function fill(field: HTMLInputElement | HTMLTextAreaElement, value: string): void {
  const prototype = field instanceof HTMLInputElement ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Une touche reçue par `target`, comme d'un clavier ; rend l'événement. */
export function press(target: Element, key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

/** La fenêtre de réglage ouverte, par son nom, ou `null`. */
export function blockWindow(label: string): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[role="dialog"][aria-label="${label}"]`);
}

/** Un éditeur de la page, lié à `doc`, sans fournisseur. */
function createEditor(doc: Y.Doc, uploadFile?: Options["uploadFile"]): MementumEditor {
  return BlockNoteEditor.create(
    withCollaboration({
      ...EDITOR_OPTIONS,
      ...(uploadFile ? { uploadFile } : {}),
      collaboration: {
        fragment: doc.getXmlFragment(FRAGMENT),
        user: { name: "Mona", color: "#66655f" },
        provider: undefined,
      },
    }),
  ) as unknown as MementumEditor;
}

/**
 * La session de la page de `m` est recréée (une baisse de droits, la corbeille, un schéma changé ;
 * ou le droit qui revient) : comme `Editor`, un éditeur neuf, lié à un document neuf qui a reçu la
 * page, prend la place de l'ancien dans la même vue. Rend le nouvel éditeur et son document ; ce que
 * l'éditeur y écrit est dans `writes`.
 */
export async function renewSession(
  m: Mounted,
  { editable, pageId }: { editable: boolean; pageId: string },
): Promise<{ editor: MementumEditor; doc: Y.Doc; writes: Uint8Array[] }> {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, Y.encodeStateAsUpdate(m.doc), REMOTE);
  const writes: Uint8Array[] = [];
  doc.on("update", (update: Uint8Array, origin: unknown) => {
    if (origin !== REMOTE) writes.push(update);
  });
  const editor = createEditor(doc);
  await act(async () => m.root.render(<EditorView editor={editor} editable={editable} pageId={pageId} />));
  await flush();
  return { editor, doc, writes };
}

/**
 * Attend que toutes les formules affichées de `m` soient mises en forme : elles le sont à l'écran,
 * l'une après l'autre, quelques millisecondes par image (`Formula`).
 */
export async function formulasShown(m: Mounted): Promise<void> {
  await until(m.host, () => m.host.querySelector(".mm-tex-pending") === null, "formules mises en forme");
}
