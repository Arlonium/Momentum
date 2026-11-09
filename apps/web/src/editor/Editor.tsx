// L'éditeur BlockNote d'une page, lié à son Y.Doc par `withCollaboration` (chemin Yjs 13
// de BlockNote 0.57, y-prosemirror 1.x).
import { syntaxHighlighter } from "@blocknote/code-block";
import { combineByGroup, filterSuggestionItems, type BlockNoteEditor } from "@blocknote/core";
import { fr } from "@blocknote/core/locales";
import { withCollaboration } from "@blocknote/core/yjs";
import { BlockNoteView } from "@blocknote/mantine";
import {
  getDefaultReactSlashMenuItems,
  SuggestionMenuController,
  useCreateBlockNote,
} from "@blocknote/react";
import {
  getMultiColumnSlashMenuItems,
  locales as multiColumnLocales,
  multiColumnDropCursor,
} from "@blocknote/xl-multi-column";
import { useCallback, useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import type { WebsocketProvider } from "y-websocket";
import type * as Y from "yjs";
import { FRAGMENT, type CollabSession } from "./collab";
import { schema, type MementumSchema } from "./schema";

export type MementumEditor = BlockNoteEditor<
  MementumSchema["blockSchema"],
  MementumSchema["inlineContentSchema"],
  MementumSchema["styleSchema"]
>;

/** Accès de développement et de test (`window.__mementum`), absent des builds de production. */
export type MementumDebug = {
  editor: MementumEditor;
  provider: WebsocketProvider;
  /** Le document lié à l'éditeur. */
  doc: Y.Doc;
  /** Première synchronisation faite (et, hors lecture seule, éditeur déverrouillé). */
  synced: boolean;
  /** Schéma du serveur différent : rien n'est jamais écrit. */
  readOnly: boolean;
};

declare global {
  interface Window {
    __mementum?: MementumDebug;
  }
}

export function Editor({ session }: { session: CollabSession }) {
  const editor = useCreateBlockNote(
    withCollaboration({
      schema,
      dictionary: { ...fr, multi_column: multiColumnLocales.fr },
      dropCursor: multiColumnDropCursor,
      extensions: [syntaxHighlighter],
      collaboration: {
        fragment: session.doc.getXmlFragment(FRAGMENT),
        user: session.user,
        // En lecture seule, ni curseur ni présence : l'onglet ne fait que lire.
        provider: session.readOnly ? undefined : session.provider,
        showCursorLabels: "activity",
      },
    }),
    [session],
  );

  const subscribe = useCallback((onChange: () => void) => session.subscribe(onChange), [session]);
  const synced = useSyncExternalStore(subscribe, () => session.synced);
  // Verrouillé jusqu'à la première synchro : la racine de la page est alors toujours celle
  // du serveur (sinon deux clients créent chacun la leur, et ProseMirror en supprime une
  // avec son texte). Une coupure réseau ensuite ne verrouille pas : les éditions faites
  // hors ligne fusionnent au retour.
  const editable = synced && !session.readOnly;

  const debug = useRef<MementumDebug | null>(null);
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    const exposed: MementumDebug = {
      editor,
      provider: session.provider,
      doc: session.doc,
      synced: false,
      readOnly: session.readOnly,
    };
    debug.current = exposed;
    window.__mementum = exposed;
    return () => {
      if (window.__mementum === exposed) delete window.__mementum;
      debug.current = null;
    };
  }, [editor, session]);
  // Après le rendu : quand `synced` passe à vrai, l'éditeur est déjà modifiable.
  useEffect(() => {
    if (debug.current) debug.current.synced = synced;
  }, [synced, editor, session]);

  const slashItems = useMemo(
    () => combineByGroup(getDefaultReactSlashMenuItems(editor), getMultiColumnSlashMenuItems(editor)),
    [editor],
  );

  return (
    <BlockNoteView
      editor={editor}
      editable={editable}
      theme="light"
      slashMenu={false}
      className="mm-editor"
    >
      <SuggestionMenuController
        triggerCharacter="/"
        getItems={async (query) => filterSuggestionItems(slashItems, query)}
      />
    </BlockNoteView>
  );
}
