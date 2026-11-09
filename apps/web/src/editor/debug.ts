// Accès de développement et de test : `window.__mementum`, posé par `Editor.tsx` quand
// `import.meta.env.DEV` est vrai, absent des builds de production.
import type { BlockNoteEditor } from "@blocknote/core";
import type { WebsocketProvider } from "y-websocket";
import type * as Y from "yjs";
import type { MementumSchema } from "./schema";

export type MementumEditor = BlockNoteEditor<
  MementumSchema["blockSchema"],
  MementumSchema["inlineContentSchema"],
  MementumSchema["styleSchema"]
>;

export type MementumDebug = {
  editor: MementumEditor;
  provider: WebsocketProvider;
  /** Le document lié à l'éditeur. */
  doc: Y.Doc;
  /**
   * Première synchronisation faite (et, hors lecture seule, éditeur déverrouillé). Reste
   * vrai après une coupure : pour attendre une resynchronisation, lire `provider.synced`.
   */
  synced: boolean;
  /** Schéma du serveur différent : rien n'est jamais écrit. */
  readOnly: boolean;
};

declare global {
  interface Window {
    __mementum?: MementumDebug;
  }
}
