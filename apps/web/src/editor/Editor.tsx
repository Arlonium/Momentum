// L'éditeur BlockNote d'une page, lié à son Y.Doc par `withCollaboration` (chemin Yjs 13
// de BlockNote 0.57, y-prosemirror 1.x). Chargé à la demande (`LazyEditor.tsx`), avec ses
// styles.
//
// Le menu d'un bloc (sa poignée, à gauche) garde les entrées de BlockNote et gagne
// « Commenter », qui ouvre le panneau des commentaires sur ce bloc ; au clavier, Ctrl+Alt+M
// (Cmd+Option+M ; la lettre tapée, pas la position de la touche) commente le bloc où est le
// curseur. Rien n'est écrit dans le document : le
// commentaire part par l'API (R134). BlockNote ne montre pas la poignée d'un éditeur en
// lecture seule : pour qui commente sans modifier, c'est la marge qui l'offre (`BlockMarks`).
import "@blocknote/mantine/style.css";
import "katex/dist/katex.min.css";
import { syntaxHighlighter } from "@blocknote/code-block";
import { combineByGroup, filterSuggestionItems } from "@blocknote/core";
import { SideMenuExtension } from "@blocknote/core/extensions";
import { fr } from "@blocknote/core/locales";
import { withCollaboration } from "@blocknote/core/yjs";
import { BlockNoteView } from "@blocknote/mantine";
import {
  BlockColorsItem,
  DragHandleMenu,
  getDefaultReactSlashMenuItems,
  RemoveBlockItem,
  SideMenu,
  SideMenuController,
  SuggestionMenuController,
  TableColumnHeaderItem,
  TableRowHeaderItem,
  useBlockNoteEditor,
  useComponentsContext,
  useCreateBlockNote,
  useDictionary,
  useExtensionState,
  type SideMenuProps,
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
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
} from "react";
import * as Y from "yjs";
import { blockOfSelection } from "../comments/dom";
import { isCommentShortcut } from "../comments/shortcut";
import { FRAGMENT, type CollabSession } from "./collab";
import type { MementumDebug } from "./debug";
import { fixtureInput, replaceNewPage } from "./fixtures";
import { guardedAwareness } from "./presenceGuard";
import { schema } from "./schema";

type Props = {
  session: CollabSession;
  /** Fixture à charger dans la page neuve (`?fixture=<nom>`), en développement seulement. */
  fixture?: string | null;
  /**
   * Commente un bloc (la personne peut commenter la page) : le menu de la poignée gagne
   * « Commenter », et Ctrl+Alt+M commente le bloc du curseur. `null` : ni l'un ni l'autre.
   */
  onComment?: ((blockId: string) => void) | null;
};

export function Editor({ session, fixture = null, onComment = null }: Props) {
  const editor = useCreateBlockNote(
    withCollaboration({
      schema,
      dictionary: { ...fr, multi_column: multiColumnLocales.fr },
      dropCursor: multiColumnDropCursor,
      extensions: [syntaxHighlighter],
      collaboration: {
        fragment: session.doc.getXmlFragment(FRAGMENT),
        user: session.user,
        // En lecture seule, ni curseur ni présence : l'onglet ne fait que lire. Sinon,
        // l'éditeur ne lit des autres que les états de présence en règle : un curseur mal
        // formé lève une exception dans y-prosemirror, et l'éditeur n'affiche alors plus ce
        // qu'écrivent les autres (`presenceGuard.ts`).
        provider: session.readOnly
          ? undefined
          : { awareness: guardedAwareness(session.provider.awareness) },
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
  // La valeur figée est un état React, remplacé pendant le rendu quand l'éditeur change
  // (motif « ajuster un état quand une prop change » de React) : elle est déterministe, là
  // où `useMemo` n'est garanti que comme une optimisation.
  const [mounted, setMounted] = useState({ editor, editable });
  if (mounted.editor !== editor) setMounted({ editor, editable });
  const mountEditable = mounted.editor === editor ? mounted.editable : editable;
  useLayoutEffect(() => {
    editor.isEditable = editable;
  }, [editor, editable]);
  // Déverrouillé juste avant que l'état du serveur ne soit appliqué, dans la même tâche (la
  // racine est donc bien celle du serveur) : la page arrive dans une vue déjà modifiable.
  // Verrouillée, ProseMirror lit la sélection du DOM après l'avoir rempli (mise en page
  // forcée), puis le déverrouillage remet en page la page entière : 2 mises en page de
  // toute la page au lieu d'une (0,3 s chacune sur 10 000 blocs).
  useEffect(() => {
    if (session.readOnly) return;
    return session.onServerState(() => {
      editor.isEditable = true;
    });
  }, [editor, session]);

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

  // Le menu de la poignée : celui de BlockNote, et « Commenter » pour qui peut commenter.
  const sideMenu = useMemo(() => {
    if (onComment === null) return undefined;
    function CommentingDragHandleMenu() {
      return <BlockMenu onComment={onComment!} />;
    }
    return function CommentingSideMenu(props: SideMenuProps) {
      return <SideMenu {...props} dragHandleMenu={CommentingDragHandleMenu} />;
    };
  }, [onComment]);

  /**
   * Ctrl+Alt+M (Cmd+Option+M) : commenter le bloc où est le curseur. La lettre tapée fait foi,
   * et AltGr garde ses caractères (`isCommentShortcut`). Le bloc est lu dans la sélection du
   * document, que le navigateur pose dès le clic : celle de BlockNote peut la suivre d'un temps.
   */
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (onComment === null || !isCommentShortcut(event) || !editor.isFocused()) return;
    event.preventDefault();
    const selected = blockOfSelection(event.currentTarget, document.getSelection());
    onComment(selected ?? editor.getTextCursorPosition().block.id);
  };

  // `data-synced` (première synchro faite) et `data-editable` (éditeur modifiable) sur le
  // conteneur, en développement comme en production : les e2e s'y fient (`waitSynced`, et
  // les budgets du navigateur, mesurés sur un build de production, R38), là où
  // `window.__mementum` n'existe qu'en développement.
  return (
    <BlockNoteView
      editor={editor}
      editable={mountEditable}
      theme="light"
      slashMenu={false}
      sideMenu={sideMenu === undefined}
      className="mm-editor"
      data-synced={String(synced)}
      data-editable={String(editable)}
      onKeyDown={onKeyDown}
    >
      <SuggestionMenuController
        triggerCharacter="/"
        getItems={async (query) => filterSuggestionItems(slashItems, query)}
      />
      {sideMenu && <SideMenuController sideMenu={sideMenu} />}
    </BlockNoteView>
  );
}

/**
 * Le menu de la poignée d'un bloc : « Commenter », puis les entrées de BlockNote, dans son ordre
 * et avec ses libellés (supprimer, couleurs, en-têtes d'un tableau).
 */
function BlockMenu({ onComment }: { onComment: (blockId: string) => void }) {
  const dictionary = useDictionary();
  return (
    <DragHandleMenu>
      <CommentItem onComment={onComment} />
      <RemoveBlockItem>{dictionary.drag_handle.delete_menuitem}</RemoveBlockItem>
      <BlockColorsItem>{dictionary.drag_handle.colors_menuitem}</BlockColorsItem>
      <TableRowHeaderItem>{dictionary.drag_handle.header_row_menuitem}</TableRowHeaderItem>
      <TableColumnHeaderItem>{dictionary.drag_handle.header_column_menuitem}</TableColumnHeaderItem>
    </DragHandleMenu>
  );
}

/** « Commenter » : le bloc dont la poignée a ouvert le menu. */
function CommentItem({ onComment }: { onComment: (blockId: string) => void }) {
  const Components = useComponentsContext()!;
  const editor = useBlockNoteEditor();
  const block = useExtensionState(SideMenuExtension, {
    editor,
    selector: (state) => state?.block,
  });
  if (block === undefined) return null;
  return (
    <Components.Generic.Menu.Item className="bn-menu-item" onClick={() => onComment(block.id)}>
      Commenter
    </Components.Generic.Menu.Item>
  );
}
