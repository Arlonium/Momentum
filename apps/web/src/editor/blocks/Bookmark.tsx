// Le signet (registre : `url`) : une carte qui montre l'hôte et l'adresse d'une page web, en lien
// (`target="_blank"`, `rel="noopener noreferrer"`). Pas d'aperçu distant (titre, image) : le serveur
// ne va chercher aucune adresse (R250).
//
// Dans l'éditeur, son adresse s'écrit par sa fenêtre (`BookmarkWindow`, « Modifier le signet », ou
// Ctrl+Alt+Entrée quand le signet est sélectionné), et seulement si c'est une adresse web entière ;
// l'adresse écrite est celle que rend `webUrl` (`https://mementum.dev` devient
// `https://mementum.dev/`). La barre de mise en forme n'offre rien sur un signet (`Toolbar.tsx`) :
// BlockNote y mettait « Remplacer le fichier », qui écrivait l'adresse sans la vérifier.
//
// D'autres chemins écrivent une adresse sans passer par cette fenêtre (R477) : le collage d'un bloc
// copié dans BlockNote (son HTML porte l'adresse telle quelle), MMD, un agent, un autre client. La
// sûreté est donc dans la vue : ce qui n'est pas une adresse web se montre en texte, isolé, jamais
// en lien ; une prop qui n'est pas une chaîne est une adresse vide.
//
// La vue du signet est dessinée sans React (`richView.ts`, R626) ; sa fenêtre, rendue à la demande
// hors du document, l'est par React.
import { createBlockSpec } from "@blocknote/core";
import { useId, useRef, useState, type FormEvent } from "react";
import { shorten } from "../../comments/dom";
import type { MementumEditor } from "../debug";
import {
  blockDraft,
  openWindowOf,
  setBlockDraft,
  toggleBlockWindow,
  watchBlockWindows,
  type BlockWindowProps,
} from "./blockWindows";
import { textProp } from "./props";
import { BOOKMARK_ICON, element, followProps, iconElement, stopEventsOutside } from "./richView";
import { hostOf, webUrl } from "./urls";
import { editableNow, watchEditable } from "./useEditable";

/** Le message d'une adresse refusée. */
export const ADDRESS_REFUSED = "Une adresse commence par http:// ou https://";

/** Une adresse qui n'est pas une adresse web n'est montrée qu'en partie. */
const MAX_RAW_CHARS = 200;

/** Dessine dans `body` ce que montre un signet de l'adresse `url`. */
function drawBody(body: HTMLElement, url: string): void {
  const href = webUrl(url);
  if (href !== null) {
    const link = element("a", "mm-bookmark-link");
    link.href = href;
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    const host = element("span", "mm-bookmark-host");
    host.dir = "auto";
    host.textContent = hostOf(href);
    const shown = element("span", "mm-bookmark-url");
    shown.dir = "auto";
    shown.textContent = href;
    link.append(host, shown);
    body.replaceChildren(link);
  } else if (url === "") {
    const empty = element("span", "mm-bookmark-empty");
    empty.textContent = "Signet sans adresse";
    body.replaceChildren(empty);
  } else {
    const raw = element("span", "mm-bookmark-raw");
    raw.dir = "auto";
    raw.textContent = shorten(url, MAX_RAW_CHARS);
    body.replaceChildren(raw);
  }
}

const BOOKMARK_PROPS = { url: { default: "" } } as const;

/**
 * La vue d'un signet : son icône, son adresse, et le bouton « Modifier » pour qui peut modifier (ou
 * « Texte non enregistré » pour qui a tapé une adresse avant de perdre le droit d'écrire, R472).
 * `live` : la vue est celle de l'éditeur (ProseMirror la détruira), pas un rendu de copie ou d'export ;
 * elle suit alors l'éditeur, modifiable ou non, la fenêtre et son brouillon.
 */
function bookmarkView(
  block: { id: string; props: Readonly<Record<string, unknown>> },
  editor: MementumEditor,
  live: boolean,
) {
  const id = block.id;
  let url = textProp(block.props, "url");
  const dom = element("div", "mm-frame mm-bookmark");
  dom.contentEditable = "false";
  const icon = element("span", "mm-frame-icon");
  icon.setAttribute("aria-hidden", "true");
  icon.append(iconElement(16, BOOKMARK_ICON));
  const body = element("span", "mm-frame-body mm-bookmark-body");
  drawBody(body, url);
  dom.append(icon, body);

  let button: HTMLButtonElement | null = null;
  /** Le bouton tel qu'il doit être : là ou non, son nom, son texte, et la fenêtre ouverte ou non. */
  const drawButton = (editable: boolean) => {
    // Un texte tapé avant de perdre le droit d'écrire reste atteignable (R472).
    const wanted = editable || blockDraft(editor, id, "bookmark") !== undefined;
    if (!wanted) {
      button?.remove();
      button = null;
      return;
    }
    if (button === null) {
      const created = element("button", "mm-button mm-button-small mm-bookmark-edit");
      created.type = "button";
      created.setAttribute("aria-haspopup", "dialog");
      created.setAttribute("data-block-window-anchor", "");
      created.addEventListener("click", () => {
        // Un clic ne donne pas le focus à un bouton partout (Safari) : la fenêtre qu'il ferme l'avait,
        // il le garde.
        created.focus();
        toggleBlockWindow(editor, { id, type: "bookmark" });
      });
      dom.append(created);
      button = created;
    }
    const label = editable ? "Modifier le signet" : "Texte non enregistré du signet";
    if (button.getAttribute("aria-label") !== label) button.setAttribute("aria-label", label);
    const text = editable ? "Modifier" : "Texte non enregistré";
    if (button.textContent !== text) button.textContent = text;
    const open = String(openWindowOf(editor)?.blockId === id);
    if (button.getAttribute("aria-expanded") !== open) button.setAttribute("aria-expanded", open);
  };

  if (!live) {
    drawButton(editableNow(editor));
    return { dom };
  }
  const editable = watchEditable(editor, (now) => drawButton(now));
  drawButton(editable.editable);
  const stopWindows = watchBlockWindows(editor, () => drawButton(editableNow(editor)));
  return {
    dom,
    // L'adresse changée (par un autre, par la fenêtre) : la vue suit en place, et le bouton qui avait
    // le focus le garde.
    update(node: { type: { name: string }; attrs: Record<string, unknown> }) {
      if (node.type.name !== "bookmark") return false;
      const next = textProp(node.attrs, "url");
      if (next !== url) {
        url = next;
        drawBody(body, url);
      }
      followProps(dom, node.attrs, BOOKMARK_PROPS);
      return true;
    },
    stopEvent: stopEventsOutside(dom),
    destroy() {
      editable.stop();
      stopWindows();
    },
  };
}

export const Bookmark = createBlockSpec(
  { type: "bookmark", propSchema: BOOKMARK_PROPS, content: "none" },
  {
    render(block, editor) {
      const live = "renderType" in this && this.renderType === "nodeView";
      return bookmarkView(block, editor as unknown as MementumEditor, live);
    },
  },
);

/**
 * La fenêtre d'un signet : un champ d'adresse. Tant qu'on n'y a rien tapé, il suit l'adresse du bloc,
 * relue par son id (un autre peut la changer pendant que la fenêtre est ouverte), et « Enregistrer »
 * ferme sans rien écrire (R473). Ce qu'on tape vit en brouillon, rangé par page et par id de bloc
 * (`setBlockDraft`), et n'est jamais remplacé par ce qui arrive d'ailleurs ; « Enregistrer » (ou
 * Entrée) écrit alors l'adresse que rend `webUrl`, ou dit pourquoi elle est refusée, sans rien écrire.
 * Échap ferme la fenêtre sans perdre le brouillon, que la rouvrir retrouve, tant que la page est
 * ouverte. Le bloc parti (supprimé, ou changé en autre chose par un autre), ou l'éditeur en lecture
 * seule, le champ reste, en lecture seule, avec ce qu'on y a tapé, ou la dernière adresse vue.
 */
export function BookmarkWindow({ editor, blockId, block, editable, close }: BlockWindowProps) {
  const id = useId();
  const [draft, setDraft] = useState(() => blockDraft(editor, blockId, "bookmark"));
  const [refused, setRefused] = useState(false);
  // La dernière adresse vue du bloc, gardée quand il part.
  const seen = useRef("");
  if (block !== null) seen.current = textProp(block.props, "url");
  const shown = draft ?? seen.current;
  const writable = block !== null && editable;
  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!writable) return;
    // Rien de tapé : rien à écrire, l'adresse du bloc reste telle qu'un autre a pu la laisser.
    if (draft === undefined) {
      close("done");
      return;
    }
    const href = webUrl(draft);
    if (href === null) {
      setRefused(true);
      return;
    }
    editor.updateBlock(blockId, { props: { url: href } });
    setBlockDraft(editor, blockId, "bookmark", undefined);
    close("done");
  };
  return (
    <form className="mm-block-window-body" onSubmit={onSubmit} noValidate>
      <label className="mm-block-window-heading" htmlFor={`${id}-url`}>
        Adresse de la page
      </label>
      <input
        id={`${id}-url`}
        className="mm-input mm-bookmark-field"
        type="text"
        inputMode="url"
        autoComplete="off"
        spellCheck={false}
        placeholder="https://…"
        dir="ltr"
        value={shown}
        readOnly={!writable}
        aria-invalid={refused || undefined}
        aria-describedby={refused ? `${id}-error` : undefined}
        data-autofocus=""
        onChange={(event) => {
          setDraft(event.target.value);
          setBlockDraft(editor, blockId, "bookmark", event.target.value);
          setRefused(false);
        }}
      />
      {refused && (
        <p id={`${id}-error`} className="mm-block-window-error" role="alert">
          {ADDRESS_REFUSED}
        </p>
      )}
      <div className="mm-block-window-actions">
        <button type="submit" className="mm-button mm-button-primary" aria-disabled={!writable || undefined}>
          Enregistrer
        </button>
      </div>
    </form>
  );
}
