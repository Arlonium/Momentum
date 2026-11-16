// Le lien vers une page (registre : `pageLink`, props `pageId` et `title` ; MMD
// `<page id="…">titre</page>`). Le titre est copié dans le document quand on pose le lien (R251, R571) :
// qui voit la page (elle est dans l'arbre de l'espace affiché, `pageDirectory.ts`) lit son titre à
// jour ; qui ne la voit pas ne lit que ce titre copié, jamais un titre venu d'ailleurs (preuve 4 du
// plan). Rien ne se lit au serveur pour un lien : l'arbre est déjà là.
//
// Le lien n'est un lien que si `pageId` est un identifiant de page (un UUID) ; sinon, le titre copié
// en texte. Un clic simple ouvre la page sans recharger l'application (`useLinkInterception`, qui
// suit les liens des îlots non modifiables de l'éditeur) ; au clavier, Ctrl+Alt+Entrée sur le lien
// sélectionné y met le focus (`focusBlockLinks`, `Toc.tsx`), et Entrée le suit. Le titre est un nœud
// de texte, isolé (`dir="auto"`).
//
// « Lien vers une page » du menu « / » ouvre un sélecteur (`PagePicker`, R572, R573) dans une fenêtre non
// modale, hors du document, près du bloc où était le curseur (`PageLinkHost`). Le bloc n'est inséré
// qu'au choix d'une page, par le geste de BlockNote (R308) : dans un bloc vide, il le devient ; après
// un texte, il est inséré après, et le curseur passe au bloc qui suit. Fermée sans choix (Échap, un
// clic dehors), la fenêtre n'écrit rien, rend le focus au texte, et garde ce qu'on y a tapé tant que
// la page est ouverte. Elle est celle de la page, pas de l'éditeur (`pageScope`, comme la fenêtre
// d'un bloc, R471) : la page passée en lecture seule pendant qu'on cherche, elle reste, avec le focus
// et la requête, le dit, et ne pose rien ; le droit revenu, le choix pose le lien.
import { insertOrUpdateBlockForSlashMenu } from "@blocknote/core";
import { createReactBlockSpec } from "@blocknote/react";
import { useCallback, useState, useSyncExternalStore } from "react";
import { blockElement, shorten } from "../../comments/dom";
import { PageIcon } from "../../icons";
import { pagePath } from "../../router";
import { isPageId, usePageTitle, type FoundPage } from "../../space/pageDirectory";
import type { MementumEditor } from "../debug";
import { BlockPopover } from "./BlockPopover";
import { focusEditor } from "./BlockWindowHost";
import { pageScope } from "./blockWindows";
import { PagePicker } from "./PagePicker";
import { textProp } from "./props";
import { useEditable } from "./useEditable";

/** Un titre de page a 500 caractères au plus ; un titre copié écrit par un autre chemin, peut-être plus. */
const MAX_TITLE_SHOWN = 500;

function PageLinkView({ block }: { block: { props: Readonly<Record<string, unknown>> } }) {
  const pageId = textProp(block.props, "pageId");
  const linked = isPageId(pageId);
  // Le titre à jour, seulement pour une page de l'arbre ; jamais lu pour ce qui n'est pas un id.
  const current = usePageTitle(linked ? pageId : "");
  // Ce qui est lu n'est pris que pour un id de page : une page publiée sous un id vide ne prête pas son titre.
  const title = shorten(
    (linked ? current : null) ?? (textProp(block.props, "title") || "Sans titre"),
    MAX_TITLE_SHOWN,
  );
  const text = (
    <span className="mm-page-link-title" dir="auto">
      {title}
    </span>
  );
  return (
    <div className="mm-frame mm-page-link" contentEditable={false}>
      <span className="mm-frame-icon" aria-hidden="true">
        <PageIcon />
      </span>
      <span className="mm-frame-body">
        {linked ? (
          <a className="mm-page-link-anchor" href={pagePath(pageId)}>
            {text}
          </a>
        ) : (
          text
        )}
      </span>
    </div>
  );
}

export const PageLink = createReactBlockSpec(
  {
    type: "pageLink",
    propSchema: { pageId: { default: "" }, title: { default: "" } },
    content: "none",
  },
  {
    render: ({ block }) => <PageLinkView block={block} />,
  },
);

/** Le nom de la fenêtre du sélecteur, et ce qu'elle dit quand la page n'est pas modifiable. */
export const PICKER_LABEL = "Lien vers une page";
export const PICKER_READ_ONLY = "Tu ne peux pas modifier cette page pour l'instant : aucun lien ne sera posé.";

/** La fenêtre ouverte : une neuve à chaque ouverture, et le bloc où était le curseur. */
type OpenPicker = { readonly id: number; readonly at: string | null };

type PickerState = { open: OpenPicker | null; query: string; readonly listeners: Set<() => void> };

/** Par page (ou par éditeur sans page) : la fenêtre ouverte et ce qu'on y a tapé. */
const pickers = new WeakMap<object, PickerState>();
let openings = 0;

function pickerOf(editor: object): PickerState {
  const scope = pageScope(editor);
  let state = pickers.get(scope);
  if (state === undefined) {
    state = { open: null, query: "", listeners: new Set() };
    pickers.set(scope, state);
  }
  return state;
}

function setOpen(editor: object, open: OpenPicker | null): void {
  const state = pickerOf(editor);
  state.open = open;
  for (const listener of [...state.listeners]) listener();
}

/** Ouvre le sélecteur du lien de page, près du bloc où est le curseur de `editor`. */
export function openPageLinkPicker(editor: MementumEditor): void {
  let at: string | null = null;
  try {
    at = editor.getTextCursorPosition().block.id;
  } catch {
    // Pas de curseur : la fenêtre se pose près de l'éditeur, le lien ira où sera le curseur.
  }
  openings += 1;
  setOpen(editor, { id: openings, at });
}

/**
 * Pose le lien vers `page` : au bloc `at` (où était le curseur quand on a ouvert le sélecteur) s'il
 * est encore là, sinon où est le curseur ; par le geste du menu « / » de BlockNote.
 */
function insertPageLink(editor: MementumEditor, at: string | null, page: FoundPage): void {
  const link = { type: "pageLink", props: { pageId: page.id, title: page.title } } as const;
  const there = at === null ? undefined : editor.getBlock(at);
  if (there !== undefined && there.content !== undefined) editor.setTextCursorPosition(there.id, "end");
  const cursor = editor.getTextCursorPosition().block;
  if (cursor.content !== undefined) {
    insertOrUpdateBlockForSlashMenu(editor, link);
    return;
  }
  // Le curseur est sur un bloc sans texte (un signet sélectionné) : le lien va après lui, et le
  // curseur au texte qui suit, un paragraphe neuf s'il n'y en a pas : la frappe ne remplace rien.
  const [inserted] = editor.insertBlocks([link], cursor, "after");
  const next = editor.getNextBlock(inserted);
  const text = next?.content !== undefined ? next : editor.insertBlocks([{ type: "paragraph" }], inserted, "after")[0];
  editor.setTextCursorPosition(text, "start");
}

/**
 * Le sélecteur n'a pas de bouton qui l'ouvre et le ferme : il est posé près du bloc où était le
 * curseur, ou de l'éditeur entier si ce bloc est parti, et un clic là le ferme comme ailleurs.
 */
const NO_OPENER = () => null;

/** Le sélecteur du lien de page ouvert dans la page de `editor`, s'il l'est. */
export function PageLinkHost({ editor }: { editor: MementumEditor }) {
  const listen = useCallback(
    (listener: () => void) => {
      const { listeners } = pickerOf(editor);
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    [editor],
  );
  const open = useSyncExternalStore(listen, () => pickerOf(editor).open);
  if (open === null) return null;
  return <PageLinkPicker key={open.id} editor={editor} open={open} />;
}

function PageLinkPicker({ editor, open }: { editor: MementumEditor; open: OpenPicker }) {
  const state = pickerOf(editor);
  const [query, setQuery] = useState(state.query);
  const editable = useEditable(editor);

  const close = (how: "escape" | "outside" | "done") => {
    setOpen(editor, null);
    if (how !== "outside") {
      focusEditor(editor);
      return;
    }
    // Le clic met le focus où il veut ; s'il ne le met nulle part, il revient au texte.
    setTimeout(() => {
      const active = document.activeElement;
      if (active === null || active === document.body) focusEditor(editor);
    }, 0);
  };
  // Rien ne se choisit dans un éditeur qui n'est pas modifiable (`disabled`, plus bas).
  const pick = (page: FoundPage) => {
    insertPageLink(editor, open.at, page);
    state.query = "";
    close("done");
  };
  const anchor = () => {
    const root = editor.domElement ?? null;
    return (root !== null && open.at !== null ? blockElement(root, open.at) : null) ?? root;
  };

  return (
    <BlockPopover label={PICKER_LABEL} anchor={anchor} opener={NO_OPENER} onClose={close}>
      {!editable && (
        <p className="mm-block-window-gone" role="status">
          {PICKER_READ_ONLY}
        </p>
      )}
      <PagePicker
        query={query}
        onQuery={(text) => {
          setQuery(text);
          state.query = text;
        }}
        onPick={pick}
        disabled={!editable}
      />
    </BlockPopover>
  );
}
