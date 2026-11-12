// L'aperçu d'une version (R135) : dans le même éditeur que la page, en lecture seule, lié à un
// `Y.Doc` neuf rempli par l'état de la version, sans fournisseur. y-prosemirror retire d'un
// document ce qu'il ne sait pas construire, dès l'affichage, et une version peut porter des blocs
// d'un autre schéma : l'aperçu n'écrit donc jamais, nulle part, ni dans le document de la page,
// ni sur le réseau.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { getVersionState } from "../api";
import { FRAGMENT } from "../editor/collab";
import "../test/hooks";
import { notFound, unreachable } from "../test/pages";
import { VersionPreview } from "./VersionPreview";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getVersionState: vi.fn(),
}));

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
  vi.mocked(getVersionState).mockReset();
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

  expect(vi.mocked(getVersionState)).toHaveBeenCalledWith("p", "v1");
  // Le même éditeur que la page, non modifiable.
  expect(shown()!.getAttribute("contenteditable")).toBe("false");
  expect(shown()!.textContent).not.toContain("Le texte d'aujourd'hui");
  // Aucune connexion, aucun canal entre onglets, et le document de la page n'a rien reçu.
  expect(sockets()).toBe(0);
  expect(channels()).toBe(0);
  expect(liveUpdates).toBe(0);
  expect(Y.encodeStateAsUpdate(live)).toEqual(before);
});

test("current_version_follows_the_page_without_writing_to_it", async () => {
  const sockets = countConstructions("WebSocket");
  const channels = countConstructions("BroadcastChannel");
  // Le document de la page porte un bloc d'un autre schéma : l'aperçu le retire chez lui seulement.
  const live = pageDoc(["Le texte d'aujourd'hui"], { foreign: true });
  const sent: Uint8Array[] = [];
  live.on("update", (update: Uint8Array, origin: unknown) => {
    if (origin !== "remote") sent.push(update);
  });

  await render(<VersionPreview pageId="p" versionId={null} current={live} />);
  await until(
    () => shown()?.textContent?.includes("Le texte d'aujourd'hui") ?? false,
    "la version actuelle",
  );
  expect(vi.mocked(getVersionState)).not.toHaveBeenCalled();
  expect(shown()!.getAttribute("contenteditable")).toBe("false");

  // Quelqu'un écrit dans la page : l'aperçu de la version actuelle le montre.
  const other = new Y.Doc();
  Y.applyUpdate(other, Y.encodeStateAsUpdate(live));
  const group = other.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement;
  const typed: Uint8Array[] = [];
  other.on("update", (update: Uint8Array) => typed.push(update));
  other.transact(() => addParagraph(group, "b_new", "Une phrase de plus"));
  await act(async () => {
    for (const update of typed) Y.applyUpdate(live, update, "remote");
  });
  await until(() => shown()?.textContent?.includes("Une phrase de plus") ?? false, "la frappe");

  expect(sockets()).toBe(0);
  expect(channels()).toBe(0);
  // Seule la frappe venue d'ailleurs est arrivée dans le document de la page.
  expect(sent).toEqual([]);
  // Le bloc d'un autre schéma y est toujours (`toString` écrit les noms en minuscules).
  expect(live.getXmlFragment(FRAGMENT).toString()).toContain("<futurewidget");
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
