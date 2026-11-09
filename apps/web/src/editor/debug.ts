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
  /** L'état Yjs complet du document lié à l'éditeur (export des fixtures). */
  encodeState: () => Uint8Array;
  /**
   * Fixture demandée par `?fixture=<nom>` (`fixtures.ts`) : `loaded` une fois chargée,
   * `refused` si la page n'est pas neuve, si la fixture n'existe pas ou si BlockNote la
   * refuse.
   */
  fixture?: "loaded" | "refused";
};

declare global {
  interface Window {
    __mementum?: MementumDebug;
  }
}
