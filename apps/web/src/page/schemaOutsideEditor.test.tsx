// Qui commente lit la page dans une session en lecture seule : avant chaque connexion de synchro,
// le schéma du serveur est relu pour lui aussi (R35, R177). Un serveur mis à jour pendant que
// l'onglet est ouvert fait alors passer les commentaires en lecture seule : le panneau cache ses
// champs, et la session, qui ne fait que recevoir, se connecte comme avant.
import type { PageNode } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getInfo, getPage } from "../api";
import { CommentsPanel } from "../comments/CommentsPanel";
import { useServerSchema } from "../editor/useServerSchema";
import { CLIENT_SCHEMA_VERSION } from "../editor/version";
import { writesOutsideEditor } from "../share/levels";
import { thread, useMemoryComments } from "../test/comments";
import "../test/hooks";
import { page } from "../test/pages";
import { usePageGate } from "./usePageGate";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getInfo: vi.fn(),
  getPage: vi.fn(),
}));

let host: HTMLDivElement;
let root: Root;
/** La vérification d'avant la prochaine connexion de synchro. */
let check: () => Promise<boolean> = async () => false;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(getInfo).mockReset();
  vi.mocked(getPage).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const info = (schemaVersion: number) =>
  ({ schemaVersion }) as unknown as Awaited<ReturnType<typeof getInfo>>;

/** Une page ouverte en lecture seule, son panneau des commentaires ouvert. */
function Opened({ node }: { node: PageNode }) {
  const schema = useServerSchema();
  const comments = useMemoryComments([thread("t")]);
  const gate = usePageGate({
    id: node.id,
    writable: false,
    writesOutsideEditor: !schema.stale && writesOutsideEditor(node.level, node.trashedAt !== null),
    checkSchema: schema.check,
    onPage: () => {},
    onGone: () => {},
    afterLogin: () => new Promise(() => {}),
  });
  check = gate.check;
  return (
    <CommentsPanel
      id="panneau"
      comments={comments}
      order={[]}
      level={node.level}
      trashed={false}
      readOnly={schema.stale}
      selfId="alice"
      target={null}
      excerpt={() => null}
      onShowBlock={() => {}}
      onTargetDone={() => {}}
      onClose={() => {}}
    />
  );
}

const textareas = () => host.querySelectorAll("textarea").length;

test("a_commenter_learns_a_new_schema_before_connecting_and_the_panel_hides_its_fields", async () => {
  const node = page("x", { level: "comment" });
  vi.mocked(getPage).mockResolvedValue(node);
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION));
  await act(async () => root.render(<Opened node={node} />));
  expect(textareas()).toBe(1);
  // Le serveur est mis à jour ; la connexion de synchro revient.
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION + 1));
  let verdict: boolean | undefined;
  await act(async () => {
    verdict = await check();
  });
  // La session se connecte (elle ne fait que recevoir) ; les commentaires ne s'écrivent plus.
  expect(verdict).toBe(true);
  expect(textareas()).toBe(0);
  expect(host.textContent).not.toContain("Répondre");
});

test("a_reader_connects_without_reading_the_schema", async () => {
  const node = page("x", { level: "read" });
  vi.mocked(getPage).mockResolvedValue(node);
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION));
  await act(async () => root.render(<Opened node={node} />));
  // La lecture du chargement.
  expect(vi.mocked(getInfo)).toHaveBeenCalledTimes(1);
  let verdict: boolean | undefined;
  await act(async () => {
    verdict = await check();
  });
  expect(verdict).toBe(true);
  expect(vi.mocked(getInfo)).toHaveBeenCalledTimes(1);
});
