// Le callout (registre : `icon`, défaut « 💡 », et `color`, défaut `default`) : un encadré, son
// icône et sa couleur. Pour qui peut modifier la page, l'icône est un bouton, « Icône et couleur du
// callout », qui ouvre sa fenêtre de réglage (`CalloutWindow`, rendue hors du document par
// `BlockWindowHost`) ; Ctrl+Alt+Entrée l'ouvre aussi depuis le texte du callout.
//
// L'icône et la couleur viennent du document, donc d'ailleurs (MMD, un agent, un autre client) :
// l'icône est du texte (`dir="auto"`), coupée à l'affichage au-delà de trois caractères tels qu'on
// les voit, entière dans l'infobulle ; la couleur ne fait que choisir une classe d'une liste
// fermée (`mm-callout-<couleur>`, fonds dans `theme.css`), jamais un attribut `style`. Une couleur
// hors de la liste est celle par défaut.
import { createReactBlockSpec } from "@blocknote/react";
import { useState, type KeyboardEvent, type ReactNode } from "react";
import { InfoIcon } from "../../icons";
import { isolate } from "../../comments/dom";
import type { MementumEditor } from "../debug";
import { toggleBlockWindow, useBlockWindow, type BlockWindowProps } from "./blockWindows";
import { textProp } from "./props";
import { useEditable } from "./useEditable";

/** Icône par défaut d'un callout (défaut du registre), affichée en icône vectorielle. */
export const DEFAULT_CALLOUT_ICON = "💡";

/** Les icônes proposées, dans l'ordre de la fenêtre. */
export const CALLOUT_ICONS = ["💡", "⚠️", "❗", "✅", "❓", "📌", "🔥", "📝"] as const;

/** Les couleurs de BlockNote, dans l'ordre de la fenêtre. */
export const CALLOUT_COLORS = [
  "default",
  "gray",
  "brown",
  "red",
  "orange",
  "yellow",
  "green",
  "blue",
  "purple",
  "pink",
] as const;
export type CalloutColor = (typeof CALLOUT_COLORS)[number];

/** Leurs noms, dans la fenêtre. */
const COLOR_NAMES: Record<CalloutColor, string> = {
  default: "Par défaut",
  gray: "Gris",
  brown: "Marron",
  red: "Rouge",
  orange: "Orange",
  yellow: "Jaune",
  green: "Vert",
  blue: "Bleu",
  purple: "Violet",
  pink: "Rose",
};

/**
 * Le fond de chaque couleur : celui des classes `mm-callout-<couleur>` de `theme.css` (un test le
 * vérifie), où le texte (`--mm-text`, #2F2E2B) tient 4,5:1 au moins.
 */
export const CALLOUT_BACKGROUNDS: Record<CalloutColor, string> = {
  default: "#F4F3EF",
  gray: "#F1F1EF",
  brown: "#F4EEEE",
  red: "#FBE4E4",
  orange: "#FBEEDB",
  yellow: "#FBF3DB",
  green: "#EDF3EC",
  blue: "#E7F3F8",
  purple: "#F6F3F9",
  pink: "#FAF1F5",
};

/** La couleur d'un callout lue dans le document : une de la liste, sinon celle par défaut. */
export function calloutColor(raw: string): CalloutColor {
  return (CALLOUT_COLORS as readonly string[]).includes(raw) ? (raw as CalloutColor) : "default";
}

/** Au-delà, l'icône est coupée à l'affichage (caractères tels qu'on les voit). */
const MAX_ICON_GRAPHEMES = 3;

let segmenter: Intl.Segmenter | null = null;

/**
 * L'icône à afficher : ses trois premiers caractères visibles, suivis de « … » si elle est plus
 * longue. La découpe s'arrête au quatrième : une icône de MMD peut porter des centaines de milliers
 * de caractères.
 */
function shownIcon(icon: string): { text: string; cut: boolean } {
  // Un seul découpeur pour tous les callouts de la page (une page peut en porter mille).
  segmenter ??= new Intl.Segmenter(undefined, { granularity: "grapheme" });
  const graphemes: string[] = [];
  for (const part of segmenter.segment(icon)) {
    graphemes.push(part.segment);
    if (graphemes.length > MAX_ICON_GRAPHEMES) break;
  }
  if (graphemes.length <= MAX_ICON_GRAPHEMES) return { text: icon, cut: false };
  return { text: `${graphemes.slice(0, MAX_ICON_GRAPHEMES).join("")}…`, cut: true };
}

/** Ce qui ne se voit pas : espaces, caractères de contrôle et de format (U+202E, U+200B…). */
const INVISIBLE = /[\p{Z}\p{Cc}\p{Cf}\s]/gu;

/**
 * L'icône d'un callout : l'icône vectorielle pour celle par défaut, et pour une icône qui ne montre
 * rien (vide, faite d'espaces ou de caractères de format) ; sinon son texte.
 */
function CalloutGlyph({ icon }: { icon: string }) {
  if (icon === DEFAULT_CALLOUT_ICON || icon.replace(INVISIBLE, "") === "") return <InfoIcon />;
  const { text, cut } = shownIcon(icon);
  return (
    <span className="mm-callout-glyph" dir="auto" title={cut ? isolate(icon) : undefined}>
      {text}
    </span>
  );
}

type ViewProps = {
  block: { id: string; type: string; props: Readonly<Record<string, unknown>> };
  editor: MementumEditor;
  contentRef: (node: HTMLElement | null) => void;
};

function CalloutView({ block, editor, contentRef }: ViewProps) {
  const icon = textProp(block.props, "icon", DEFAULT_CALLOUT_ICON);
  const color = calloutColor(textProp(block.props, "color", "default"));
  const editable = useEditable(editor);
  const open = useBlockWindow(editor, block.id);
  return (
    <div className={`mm-callout mm-callout-${color}`}>
      {editable ? (
        <button
          type="button"
          className="mm-callout-icon mm-callout-button"
          contentEditable={false}
          aria-label="Icône et couleur du callout"
          aria-haspopup="dialog"
          aria-expanded={open}
          data-block-window-anchor=""
          onClick={(event) => {
            // Un clic ne donne pas le focus à un bouton partout (Safari) : la fenêtre qu'il ferme
            // l'avait, il le garde.
            event.currentTarget.focus();
            toggleBlockWindow(editor, block);
          }}
        >
          <CalloutGlyph icon={icon} />
        </button>
      ) : (
        <span className="mm-callout-icon" contentEditable={false}>
          <CalloutGlyph icon={icon} />
        </span>
      )}
      <div className="mm-callout-content" ref={contentRef} />
    </div>
  );
}

export const Callout = createReactBlockSpec(
  {
    type: "callout",
    propSchema: { icon: { default: DEFAULT_CALLOUT_ICON }, color: { default: "default" } },
    content: "inline",
  },
  {
    render: ({ block, editor, contentRef }) => (
      <CalloutView block={block} editor={editor as unknown as MementumEditor} contentRef={contentRef} />
    ),
  },
);

type Choice = { value: string; label: ReactNode };

/**
 * Une liste de choix au clavier (`ul` `role="listbox"`) : les flèches, Début et Fin déplacent le
 * focus sans rien écrire ; Entrée, Espace ou un clic choisissent. Un seul élément de la liste est
 * dans l'ordre de Tab (le choisi, ou le premier). `disabled` : le bloc n'est plus là, plus rien ne
 * s'écrit, mais la liste reste, et le focus avec elle.
 */
function ChoiceList({
  label,
  choices,
  selected,
  onChoose,
  disabled,
  autoFocus,
  className,
}: {
  label: string;
  choices: Choice[];
  selected: string;
  onChoose: (value: string) => void;
  disabled: boolean;
  autoFocus?: boolean;
  className: string;
}) {
  const start = Math.max(
    0,
    choices.findIndex((c) => c.value === selected),
  );
  const [active, setActive] = useState(start);
  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const items = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="option"]')];
    // L'élément qui a le focus fait foi : deux touches peuvent arriver avant le rendu suivant.
    const at = Math.max(0, items.indexOf(event.target as HTMLElement));
    const move = (to: number) => {
      event.preventDefault();
      const next = (to + choices.length) % choices.length;
      setActive(next);
      items[next]?.focus();
    };
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        return move(at + 1);
      case "ArrowLeft":
      case "ArrowUp":
        return move(at - 1);
      case "Home":
        return move(0);
      case "End":
        return move(choices.length - 1);
      case "Enter":
      case " ":
        event.preventDefault();
        setActive(at);
        if (!disabled) onChoose(choices[at].value);
    }
  };
  return (
    <ul role="listbox" aria-label={label} aria-disabled={disabled || undefined} className={className} onKeyDown={onKeyDown}>
      {choices.map((choice, index) => (
        <li
          key={choice.value}
          role="option"
          aria-selected={choice.value === selected}
          tabIndex={index === active ? 0 : -1}
          data-autofocus={autoFocus && index === active ? "" : undefined}
          onClick={() => {
            setActive(index);
            if (!disabled) onChoose(choice.value);
          }}
        >
          {choice.label}
        </li>
      ))}
    </ul>
  );
}

const ICON_CHOICES: Choice[] = CALLOUT_ICONS.map((icon) => ({ value: icon, label: icon }));
const COLOR_CHOICES: Choice[] = CALLOUT_COLORS.map((color) => ({
  value: color,
  label: (
    <>
      <span className={`mm-swatch mm-callout-${color}`} aria-hidden="true" />
      {COLOR_NAMES[color]}
    </>
  ),
}));

/**
 * La fenêtre d'un callout : son icône, puis sa couleur. Un choix s'écrit aussitôt dans le bloc (un
 * geste, aucun texte tapé), par l'id du bloc ; la fenêtre reste ouverte, Échap la ferme.
 */
export function CalloutWindow({ editor, blockId, block, editable }: BlockWindowProps) {
  const props = block?.props ?? {};
  const icon = textProp(props, "icon", DEFAULT_CALLOUT_ICON);
  const color = calloutColor(textProp(props, "color", "default"));
  const disabled = block === null || !editable;
  // `ChoiceList` n'appelle rien quand `disabled` est vrai (le bloc parti, l'éditeur en lecture seule).
  const write = (change: { icon: string } | { color: string }) => {
    editor.updateBlock(blockId, { props: change });
  };
  return (
    <div className="mm-block-window-body">
      <p className="mm-block-window-heading" aria-hidden="true">
        Icône
      </p>
      <ChoiceList
        label="Icône"
        className="mm-icon-choices"
        choices={ICON_CHOICES}
        selected={icon}
        onChoose={(value) => write({ icon: value })}
        disabled={disabled}
        autoFocus
      />
      <p className="mm-block-window-heading" aria-hidden="true">
        Couleur
      </p>
      <ChoiceList
        label="Couleur"
        className="mm-color-choices"
        choices={COLOR_CHOICES}
        selected={color}
        onChoose={(value) => write({ color: value })}
        disabled={disabled}
      />
    </div>
  );
}
