import * as Y from "yjs";
import { WebsocketProvider } from "y-websocket";

export const FRAGMENT = "document-store";
export const SERVER = "ws://localhost:8080/sync";

const params = new URLSearchParams(location.search);
export const pageId = params.get("page") ?? "demo";
export const userName = params.get("user") ?? "Anonyme";
export const fixtureName = params.get("fixture");

const COLORS = ["#B4462B", "#2B59C3", "#23704F", "#6B4FBB", "#C27A12"];

function colorFor(name: string): string {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return COLORS[h % COLORS.length];
}

/** État Yjs complet en base64 (pour exporter les fixtures). */
function exportState(doc: Y.Doc): string {
  const bytes = Y.encodeStateAsUpdate(doc);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

export function createCollab() {
  const t0 = performance.now();
  const doc = new Y.Doc();
  const provider = new WebsocketProvider(SERVER, pageId, doc);
  const fragment = doc.getXmlFragment(FRAGMENT);
  const user = { name: userName, color: colorFor(userName) };
  const collab = { t0, tSynced: 0, doc, provider, fragment, user, exportState: () => exportState(doc) };
  provider.once("sync", () => {
    collab.tSynced = performance.now();
  });
  return collab;
}

export type Collab = ReturnType<typeof createCollab>;
