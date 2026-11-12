// L'aperçu d'une version (R135) : dans le même éditeur que la page, en lecture seule, lié à un
// `Y.Doc` neuf rempli par l'état de la version, sans fournisseur. y-prosemirror retire d'un
// document ce qu'il ne sait pas construire, dès l'affichage, et une version peut porter des blocs
// d'un autre schéma : l'aperçu n'écrit donc jamais, nulle part, ni dans le document de la page,
// ni sur le réseau. Et il ne pose rien sur le document de la page : la version actuelle est une
// copie figée (R192), et une exception de l'éditeur de l'aperçu ne peut pas remonter dans une
// transaction de la session.
import type { Version } from "@mementum/protocol";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { getVersions, getVersionState } from "../api";
import { FRAGMENT, type PageEvent, type SyncStatus } from "../editor/collab";
import { author } from "../test/comments";
import { deferred, type Deferred } from "../test/hooks";
import { notFound, page, unreachable } from "../test/pages";
import { HistoryDialog } from "./HistoryDialog";
import { VersionPreview } from "./VersionPreview";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getVersions: vi.fn(),
  getVersionState: vi.fn(),
}));

/** Les documents que l'éditeur de l'aperçu a montrés, dans l'ordre. */
const previews = vi.hoisted(() => ({ docs: [] as unknown[] }));

// Le vrai éditeur de l'aperçu, dont le test retient le document.
vi.mock("../editor/Editor", async (original) => {
  const real = await original<typeof import("../editor/Editor")>();
  return {
    ...real,
    PreviewEditor: (props: { doc: Y.Doc }) => {
      if (!previews.docs.includes(props.doc)) previews.docs.push(props.doc);
      return createElement(real.PreviewEditor, props);
    },
  };
});

/** Les documents montrés par l'aperçu. */
const previewDocs = () => previews.docs as Y.Doc[];

/**
 * Le document d'une page, tel que BlockNote l'écrit : `blockGroup > blockContainer > paragraph`.
 * `foreign` : un bloc d'un type que BlockNote ne connaît pas (celui d'une version plus récente).
 */
function pageDoc(texts: string[], { foreign = false } = {}): Y.Doc {
  const doc = new Y.Doc();
  const fragment = doc.getXmlFragment(FRAGMENT);
  doc.transact(() => {
    const group = new Y.XmlElement("blockGroup");
    fragment.insert(0, [group]);
    texts.forEach((text, i) => addParagraph(group, `b${i}`, text));
    if (foreign) {
      const container = new Y.XmlElement("blockContainer");
      group.insert(group.length, [container]);
      container.setAttribute("id", "b_future");
      const widget = new Y.XmlElement("futureWidget");
      container.insert(0, [widget]);
      widget.setAttribute("a", "1");
    }
  });
  return doc;
}

function addParagraph(group: Y.XmlElement, id: string, text: string): void {
  const container = new Y.XmlElement("blockContainer");
  group.insert(group.length, [container]);
  container.setAttribute("id", id);
  const paragraph = new Y.XmlElement("paragraph");
  container.insert(0, [paragraph]);
  const content = new Y.XmlText();
  paragraph.insert(0, [content]);
  content.insert(0, text);
}

/** Compte les objets que la page construit avec le constructeur global `name`. */
function countConstructions(name: "WebSocket" | "BroadcastChannel"): () => number {
  let count = 0;
  vi.stubGlobal(
    name,
    class {
      constructor() {
        count += 1;
      }
      close() {}
      postMessage() {}
      addEventListener() {}
      removeEventListener() {}
    },
  );
  return () => count;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  previews.docs = [];
  vi.mocked(getVersionState).mockReset();
  vi.mocked(getVersions).mockReset();
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  // Mantine lit le thème du système ; jsdom n'a pas `matchMedia`.
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
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const render = (element: React.ReactElement) => act(async () => root.render(element));

/** Attend que `check` soit vrai, en laissant React et les promesses suivre (borne large). */
async function until(check: () => boolean, what: string): Promise<void> {
  const end = Date.now() + 5_000;
  while (!check()) {
    if (Date.now() > end) throw new Error(`toujours pas : ${what}`);
    await act(async () => {
      await new Promise((done) => setTimeout(done, 10));
    });
  }
}

const shown = () => host.querySelector<HTMLElement>(".mm-preview-editor .bn-editor");

test("preview_never_writes", async () => {
  const sockets = countConstructions("WebSocket");
  const channels = countConstructions("BroadcastChannel");
  // Le document de la session vivante : rien ne doit y arriver pendant l'aperçu.
  const live = pageDoc(["Le texte d'aujourd'hui"]);
  const before = Y.encodeStateAsUpdate(live);
  let liveUpdates = 0;
  live.on("update", () => {
    liveUpdates += 1;
  });
  // La version porte un bloc d'un autre schéma, que y-prosemirror retire du document affiché.
  vi.mocked(getVersionState).mockResolvedValue(
    Y.encodeStateAsUpdate(pageDoc(["Le texte d'hier"], { foreign: true })),
  );

  await render(<VersionPreview pageId="p" versionId="v1" current={live} />);
  await until(() => shown()?.textContent?.includes("Le texte d'hier") ?? false, "l'aperçu");

  expect(vi.mocked(getVersionState)).toHaveBeenCalledWith("p", "v1", expect.any(AbortSignal));
  // Le même éditeur que la page, non modifiable.
  expect(shown()!.getAttribute("contenteditable")).toBe("false");
  expect(shown()!.textContent).not.toContain("Le texte d'aujourd'hui");
  // Aucune connexion, aucun canal entre onglets, et le document de la page n'a rien reçu.
  expect(sockets()).toBe(0);
  expect(channels()).toBe(0);
  expect(liveUpdates).toBe(0);
  expect(Y.encodeStateAsUpdate(live)).toEqual(before);
});

/** Une frappe venue d'ailleurs : un paragraphe de plus, appliqué au document de la page. */
function typedElsewhere(live: Y.Doc, id: string, text: string): void {
  const other = new Y.Doc();
  Y.applyUpdate(other, Y.encodeStateAsUpdate(live));
  const group = other.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement;
  const typed: Uint8Array[] = [];
  other.on("update", (update: Uint8Array) => typed.push(update));
  other.transact(() => addParagraph(group, id, text));
  for (const update of typed) Y.applyUpdate(live, update, "remote");
}

test("current_version_is_a_copy_taken_when_chosen_that_writes_nothing", async () => {
  const sockets = countConstructions("WebSocket");
  const channels = countConstructions("BroadcastChannel");
  // Le document de la page porte un bloc d'un autre schéma : l'aperçu le retire chez lui seulement.
  const live = pageDoc(["Le texte d'aujourd'hui"], { foreign: true });
  const sent: Uint8Array[] = [];
  live.on("update", (update: Uint8Array, origin: unknown) => {
    if (origin !== "remote") sent.push(update);
  });

  await render(<VersionPreview pageId="p" versionId={null} current={live} snapshot={1} />);
  await until(
    () => shown()?.textContent?.includes("Le texte d'aujourd'hui") ?? false,
    "la version actuelle",
  );
  expect(vi.mocked(getVersionState)).not.toHaveBeenCalled();
  expect(shown()!.getAttribute("contenteditable")).toBe("false");
  expect(previewDocs()[0]).not.toBe(live);

  // Quelqu'un écrit dans la page : la copie, figée, ne le montre pas…
  await act(async () => typedElsewhere(live, "b_new", "Une phrase de plus"));
  await act(async () => {
    await new Promise((done) => setTimeout(done, 50));
  });
  expect(shown()!.textContent).not.toContain("Une phrase de plus");
  // … jusqu'à ce qu'on choisisse de nouveau la version actuelle : une copie neuve est prise.
  await render(<VersionPreview pageId="p" versionId={null} current={live} snapshot={2} />);
  await until(() => shown()?.textContent?.includes("Une phrase de plus") ?? false, "la copie neuve");
  expect(previewDocs()).toHaveLength(2);
  expect(previewDocs()[0].isDestroyed).toBe(true);

  expect(sockets()).toBe(0);
  expect(channels()).toBe(0);
  // Seule la frappe venue d'ailleurs est arrivée dans le document de la page.
  expect(sent).toEqual([]);
  // Le bloc d'un autre schéma y est toujours (`toString` écrit les noms en minuscules).
  expect(live.getXmlFragment(FRAGMENT).toString()).toContain("<futurewidget");
});

test("an_exception_in_the_preview_never_blocks_the_page_document", async () => {
  const live = pageDoc(["Le texte d'aujourd'hui"]);
  await render(<VersionPreview pageId="p" versionId={null} current={live} snapshot={1} />);
  await until(() => previewDocs().length === 1 && shown() !== null, "la version actuelle");
  // Un observateur de l'aperçu qui lève : ce que ferait y-prosemirror sur un changement qu'il ne
  // sait pas montrer.
  previewDocs()[0].getXmlFragment(FRAGMENT).observeDeep(() => {
    throw new Error("aperçu en erreur");
  });
  let updates = 0;
  let observed = 0;
  live.on("update", () => {
    updates += 1;
  });
  live.getXmlFragment(FRAGMENT).observeDeep(() => {
    observed += 1;
  });

  // Deux frappes venues d'ailleurs : aucune exception ne remonte dans les transactions de la page,
  // qui émet encore ses mises à jour (elles partiraient au serveur) et appelle ses observateurs
  // (son éditeur les afficherait).
  await act(async () => {
    expect(() => typedElsewhere(live, "b_1", "Première frappe")).not.toThrow();
    expect(() => typedElsewhere(live, "b_2", "Seconde frappe")).not.toThrow();
  });
  expect(updates).toBe(2);
  expect(observed).toBe(2);
  // Et encore après la fermeture de l'aperçu.
  await act(async () => root.unmount());
  root = createRoot(host);
  (live.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement).insert(0, [new Y.XmlElement("blockContainer")]);
  expect(updates).toBe(3);
  expect(observed).toBe(3);
});

/**
 * Les écouteurs posés sur un document : ceux du document lui-même (`update`, `destroy`…), et les
 * observateurs de son fragment (`observe`, `observeDeep`).
 */
function listenersOn(doc: Y.Doc): number {
  const observed = doc as unknown as { _observers: Map<string, Set<unknown>> };
  const fragment = doc.getXmlFragment(FRAGMENT) as unknown as {
    _eH: { l: unknown[] };
    _dEH: { l: unknown[] };
  };
  let count = fragment._eH.l.length + fragment._dEH.l.length;
  for (const listeners of observed._observers.values()) count += listeners.size;
  return count;
}

test("the_history_leaves_no_listener_on_the_page_document", async () => {
  const live = pageDoc(["Le texte d'aujourd'hui"]);
  const listeners = new Set<() => void>();
  const events = new Set<(event: PageEvent) => void>();
  const session = {
    synced: true,
    status: "connected" as SyncStatus,
    roster: [],
    provider: { doc: live },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    onPageEvent(listener: (event: PageEvent) => void) {
      events.add(listener);
      return () => events.delete(listener);
    },
  };
  const versions: Version[] = [
    { id: "v1", createdAt: Date.now() - 60_000, reason: "periodic", authors: [author("alice")] },
  ];
  vi.mocked(getVersions).mockResolvedValue(versions);
  vi.mocked(getVersionState).mockResolvedValue(Y.encodeStateAsUpdate(pageDoc(["Le texte d'hier"])));
  const before = listenersOn(live);
  const item = (n: number) =>
    host.querySelectorAll<HTMLButtonElement>('ul[aria-label="Versions de la page"] > li > button')[n];

  // Ouvrir l'historique : la version actuelle est montrée.
  await render(
    <HistoryDialog
      node={page("p")}
      session={session}
      readOnly={false}
      selfId="alice"
      onRefused={() => {}}
      onClose={() => {}}
    />,
  );
  await until(() => shown()?.textContent?.includes("Le texte d'aujourd'hui") ?? false, "l'ouverture");
  expect(listenersOn(live)).toBe(before);
  // Parcourir : une version, puis la version actuelle de nouveau.
  await act(async () => item(1).click());
  await until(() => shown()?.textContent?.includes("Le texte d'hier") ?? false, "la version");
  expect(listenersOn(live)).toBe(before);
  await act(async () => item(0).click());
  await until(() => shown()?.textContent?.includes("Le texte d'aujourd'hui") ?? false, "le retour");
  expect(listenersOn(live)).toBe(before);
  // Fermer.
  await act(async () => root.unmount());
  root = createRoot(host);
  expect(listenersOn(live)).toBe(before);
  // Chaque aperçu quitté, et le dernier à la fermeture, est détruit.
  expect(previewDocs()).toHaveLength(3);
  expect(previewDocs().every((doc) => doc.isDestroyed)).toBe(true);
});

test("a_state_that_arrives_after_another_choice_is_not_shown_and_its_download_is_cancelled", async () => {
  const answers: { signal: AbortSignal | undefined; answer: Deferred<Uint8Array> }[] = [];
  vi.mocked(getVersionState).mockImplementation((_page, _version, signal) => {
    const answer = deferred<Uint8Array>();
    answers.push({ signal, answer });
    return answer.promise;
  });
  await render(<VersionPreview pageId="p" versionId="v1" current={null} />);
  await until(() => answers.length === 1, "la première lecture");
  // On choisit une autre version avant que la première n'arrive : son téléchargement est annulé.
  await render(<VersionPreview pageId="p" versionId="v2" current={null} />);
  await until(() => answers.length === 2, "la seconde lecture");
  expect(answers[0].signal?.aborted).toBe(true);
  expect(answers[1].signal?.aborted).toBe(false);
  // La réponse de la première arrive quand même : elle n'est pas montrée, ni gardée.
  await act(async () => answers[0].answer.resolve(Y.encodeStateAsUpdate(pageDoc(["Première"]))));
  await act(async () => {
    await new Promise((done) => setTimeout(done, 50));
  });
  expect(shown()).toBeNull();
  expect(host.textContent).toContain("Chargement de la version…");
  await act(async () => answers[1].answer.resolve(Y.encodeStateAsUpdate(pageDoc(["Seconde"]))));
  await until(() => shown()?.textContent?.includes("Seconde") ?? false, "la seconde version");
  expect(shown()!.textContent).not.toContain("Première");
  expect(previewDocs()).toHaveLength(1);
});

test("a_version_whose_state_cannot_be_read_says_so", async () => {
  vi.mocked(getVersionState).mockResolvedValue(new Uint8Array([0xff, 0xff, 0xff]));
  await render(<VersionPreview pageId="p" versionId="v1" current={null} />);
  await until(() => host.textContent?.includes("Cette version n'a pas pu être lue.") ?? false, "le refus");
  expect(shown()).toBeNull();
});

test("a_failed_state_read_says_why_and_is_done_again_on_request", async () => {
  vi.mocked(getVersionState).mockRejectedValueOnce(unreachable());
  await render(<VersionPreview pageId="p" versionId="v1" current={null} />);
  await until(() => host.querySelector('[role="alert"]') !== null, "l'erreur");
  expect(host.querySelector('[role="alert"]')!.textContent).toContain(
    "Le serveur Mementum ne répond pas.",
  );
  vi.mocked(getVersionState).mockResolvedValue(Y.encodeStateAsUpdate(pageDoc(["Revenue"])));
  const retry = [...host.querySelectorAll("button")].find((b) => b.textContent === "Réessayer")!;
  await act(async () => retry.click());
  await until(() => shown()?.textContent?.includes("Revenue") ?? false, "l'aperçu");
  expect(vi.mocked(getVersionState)).toHaveBeenCalledTimes(2);
});

test("a_version_the_server_no_longer_has_is_reported", async () => {
  const onMissing = vi.fn();
  vi.mocked(getVersionState).mockRejectedValue(notFound());
  await render(<VersionPreview pageId="p" versionId="v1" current={null} onMissing={onMissing} />);
  await until(() => onMissing.mock.calls.length > 0, "le signalement");
  expect(shown()).toBeNull();
});
