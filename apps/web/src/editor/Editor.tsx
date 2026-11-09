// L'éditeur BlockNote d'une page, lié à son Y.Doc par `withCollaboration` (chemin Yjs 13
// de BlockNote 0.57, y-prosemirror 1.x).
import { syntaxHighlighter } from "@blocknote/code-block";
import { combineByGroup, filterSuggestionItems } from "@blocknote/core";
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
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from "react";
import * as Y from "yjs";
import { FRAGMENT, type CollabSession } from "./collab";
import type { MementumDebug } from "./debug";
import { fixtureInput, replaceNewPage } from "./fixtures";
import { schema } from "./schema";

type Props = {
  session: CollabSession;
  /** Fixture à charger dans la page neuve (`?fixture=<nom>`), en développement seulement. */
  fixture?: string | null;
};

export function Editor({ session, fixture = null }: Props) {
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
  // BlockNote 0.57 démonte puis remonte toute la vue ProseMirror quand la prop `editable`
  // de `BlockNoteView` change : la page entière était reconstruite juste après la première
  // synchro (1 s sur 10 000 blocs). La prop ne sert donc qu'au montage de la vue de chaque
  // éditeur (sa valeur est figée par éditeur), et l'état suit ensuite par `isEditable`,
  // sans remonter la vue. Monter la vue avec le bon état garde aussi `tabindex="0"`, que
  // TipTap retire quand l'éditeur repasse en lecture seule.
  const mountEditable = useMemo(() => editable, [editor]);
  useLayoutEffect(() => {
    editor.isEditable = editable;
  }, [editor, editable]);

  const debug = useRef<MementumDebug | null>(null);
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    const exposed: MementumDebug = {
      editor,
      provider: session.provider,
      doc: session.doc,
      synced: false,
      readOnly: session.readOnly,
      encodeState: () => Y.encodeStateAsUpdate(session.doc),
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
  // Après la première synchro, comme une frappe : la racine de la page est celle du serveur.
  useEffect(() => {
    if (!import.meta.env.DEV || !fixture || !synced || session.readOnly) return;
    let cancelled = false;
    const settle = (loaded: boolean) => {
      if (!cancelled && debug.current) debug.current.fixture = loaded ? "loaded" : "refused";
    };
    fixtureInput(fixture)
      .then((blocks) => settle(!cancelled && blocks !== null && replaceNewPage(editor, blocks)))
      .catch((error: unknown) => {
        console.error(`Fixture ${fixture} refusée par BlockNote :`, error);
        settle(false);
      });
    return () => {
      cancelled = true;
    };
  }, [fixture, synced, editor, session]);

  const slashItems = useMemo(
    () => combineByGroup(getDefaultReactSlashMenuItems(editor), getMultiColumnSlashMenuItems(editor)),
    [editor],
  );

  return (
    <BlockNoteView
      editor={editor}
      editable={mountEditable}
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
