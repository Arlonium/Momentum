// Les menus `@` et `[[` de l'éditeur (Task C4, R251, R618 à R621) : ceux de BlockNote
// (`SuggestionMenuController`, qui ouvre le menu à la frappe du déclencheur, quel que soit le clavier,
// suit ce qu'on tape, et tient les flèches, Entrée et Échap), avec nos lignes et notre liste.
//
// - `@` propose des pages (celles de l'arbre du compte, `findPages` : aucune autre ne peut y être),
//   des personnes (`loadPeople` : les membres de l'espace pour un membre, les comptes qui ont la page
//   ouverte pour un invité ; lues à l'ouverture du menu, jamais avant) et une date (`suggestDates`) ;
//   huit lignes au plus par groupe.
// - `[[` propose des pages, et, en dernier, « Créer la page « … » » : une sous-page de la page ouverte,
//   créée une seule fois, puis mentionnée là où l'on avait tapé.
//
// Ce qui est posé : une mention (`kind`, `id`, et le libellé copié : le titre de la page à cet
// instant, le nom, la date), suivie d'une espace. Choisir retire du texte le déclencheur et ce qu'on
// avait tapé après lui ; Échap ferme le menu et laisse ce texte. Les menus restent fermés dans un
// bloc de code. Une mention ne prévient personne et ne donne aucun droit.
//
// Un refus du serveur à la création se dit tel quel, dans un message hors du document, qui nomme le
// titre tapé ; rien n'est posé, et la page est relue.
import type { Role } from "@mementum/protocol";
import { trackPosition } from "@blocknote/core";
import { SuggestionMenu } from "@blocknote/core/extensions";
import { SuggestionMenuController, type SuggestionMenuProps } from "@blocknote/react";
import type { Transaction } from "prosemirror-state";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPage, isSessionLost } from "../api";
import { focusEditor } from "../editor/blocks/BlockWindowHost";
import type { RosterUser } from "../editor/collab";
import type { MementumEditor } from "../editor/debug";
import { CloseIcon, PageIcon, PlusIcon } from "../icons";
import { messageOf } from "../session/forms";
import { findPages, pageTitle } from "../space/pageDirectory";
import { firstClick } from "../ui/firstClick";
import { dateId, dateLabel, suggestDates } from "./dates";
import { loadPeople } from "./people";

/** Ce que les menus savent de la page ouverte et de qui la lit. */
export type MentionContext = {
  /** L'espace de la page, et le rôle du compte dans cet espace. */
  spaceId: string;
  role: Role;
  /** La page ouverte : `[[` y crée une sous-page. */
  pageId: string;
  /** Les comptes qui ont la page ouverte, au moment où on le demande (`session.roster`). */
  roster: () => readonly RosterUser[];
  /** Le compte peut créer une sous-page ici (il peut modifier la page). */
  canCreate: boolean;
  /** Le serveur a refusé une création : un droit, ou la page, ont changé ; elle est à relire. */
  onRefused?: () => void;
};

/** Combien de lignes par groupe, au plus. */
export const MAX_PER_GROUP = 8;

export const GROUP_PAGES = "Pages";
export const GROUP_PEOPLE = "Personnes";
export const GROUP_DATES = "Dates";

/** Ce qu'une mention porte dans le document. */
type MentionProps = { kind: "page" | "user" | "date"; id: string; label: string };

/** Une ligne d'un menu : une mention à poser, ou la page à créer. */
export type MentionItem = {
  /** Unique dans le menu. */
  key: string;
  group: string;
  title: string;
  /** Ce qui précise la ligne (« Demain »), ou rien. */
  detail: string | null;
} & ({ mention: MentionProps } | { create: string });

/** Sans accents ni casse, pour comparer un nom à ce qu'on tape. */
function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase("fr");
}

function pageItems(query: string): MentionItem[] {
  return findPages(query, MAX_PER_GROUP).map((page) => ({
    key: `page:${page.id}`,
    group: GROUP_PAGES,
    title: page.title,
    detail: null,
    mention: { kind: "page", id: page.id, label: page.title },
  }));
}

/** Les lignes du menu `@` pour `query`, un jour `today`. */
export async function atItems(query: string, context: MentionContext, today: Date): Promise<MentionItem[]> {
  const wanted = fold(query.trim());
  const people = (await loadPeople(context.spaceId, context.role, context.roster()))
    .filter((person) => fold(person.name).includes(wanted))
    .slice(0, MAX_PER_GROUP)
    .map(
      (person): MentionItem => ({
        key: `user:${person.id}`,
        group: GROUP_PEOPLE,
        title: person.name,
        detail: null,
        mention: { kind: "user", id: person.id, label: person.name },
      }),
    );
  const dates = suggestDates(query, today)
    .slice(0, MAX_PER_GROUP)
    .map(
      ({ date, said }): MentionItem => ({
        key: `date:${dateId(date)}`,
        group: GROUP_DATES,
        title: dateLabel(date),
        detail: said,
        mention: { kind: "date", id: dateId(date), label: dateLabel(date) },
      }),
    );
  // Les pages sont lues en dernier, après l'attente des personnes : l'arbre du moment.
  return [...pageItems(query), ...people, ...dates];
}

/** Les lignes du menu `[[` pour `query`. */
export function bracketItems(query: string, context: MentionContext): MentionItem[] {
  const title = query.trim();
  const items = pageItems(query);
  if (context.canCreate && title !== "") {
    items.push({ key: "create", group: GROUP_PAGES, title: `Créer la page « ${title} »`, detail: null, create: title });
  }
  return items;
}

/** Ce que dit un refus de création, et une page créée dont la mention n'a pas trouvé sa place. */
export const createRefused = (title: string, message: string) => `La page « ${title} » n'a pas pu être créée. ${message}`;
export const createdNotPlaced = (title: string) =>
  `La page « ${title} » est créée, mais sa mention n'a pas pu être posée : l'endroit où tu l'avais demandée n'est plus là, ou la page n'est plus modifiable.`;

/** Les déclencheurs des deux menus. */
const AT = "@";
const BRACKETS = "[[";

/**
 * Un menu de mention est-il ouvert dans `editor` ? Tant qu'il l'est, ce qu'on tape lui appartient :
 * « / », « : » et « @ » sont des caractères d'une date (`01/12`) ou d'un titre (« Entrées/sorties : le
 * plan »), pas le déclencheur d'un autre menu, qui prendrait sa place (R620). `Editor.tsx` le donne
 * en `shouldOpen` au menu « / » et au sélecteur d'emoji.
 */
export function mentionMenuOpen(editor: MementumEditor): boolean {
  const state = editor.getExtension(SuggestionMenu)?.store.state;
  return state !== undefined && state.show && (state.triggerCharacter === AT || state.triggerCharacter === BRACKETS);
}

/** Les menus restent fermés dans un bloc de code : `@` et `[[` y sont du code. */
function outsideCode(tr: Transaction): boolean {
  return !tr.selection.$from.parent.type.spec.code;
}

/** Pose `mention` au curseur de `editor`, suivie d'une espace. */
function insertMention(editor: MementumEditor, mention: MentionProps): void {
  editor.insertInlineContent([{ type: "mention", props: mention }, " "]);
}

/**
 * Pose `mention` à la place `pos` du document de `editor`, suivie d'une espace, si un texte s'écrit
 * encore là ; rend faux sinon (la place est partie, ou l'éditeur n'est plus modifiable).
 */
function insertMentionAt(editor: MementumEditor, pos: number, mention: MentionProps): boolean {
  if (!editor.isEditable || editor.prosemirrorView === undefined) return false;
  try {
    return editor.transact((tr) => {
      const parent = tr.doc.resolve(pos).parent;
      const node = tr.doc.type.schema.nodes.mention;
      if (!parent.inlineContent || parent.type.spec.code || node === undefined) return false;
      tr.insert(pos, [node.create(mention), tr.doc.type.schema.text(" ")]);
      return true;
    });
  } catch {
    return false;
  }
}

type Props = { editor: MementumEditor; context: MentionContext };

/** Les menus `@` et `[[` de l'éditeur `editor`, et le message d'une création refusée. */
export function MentionMenus({ editor, context }: Props) {
  const [notice, setNotice] = useState<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  /** Une création à la fois. */
  const creating = useRef(false);

  const getAtItems = useCallback((query: string) => atItems(query, context, new Date()), [context]);
  const getBracketItems = useCallback(async (query: string) => bracketItems(query, context), [context]);

  const create = useCallback(
    async (title: string) => {
      if (creating.current) return;
      creating.current = true;
      setNotice(null);
      // Là où l'on avait tapé `[[` : ce qu'on tape en attendant le serveur vient après la mention.
      const place = trackPosition(editor, editor.prosemirrorState.selection.from, "left");
      try {
        const node = await createPage({ parentId: context.pageId, title });
        const placed = insertMentionAt(editor, place(), { kind: "page", id: node.id, label: node.title });
        if (!placed && mounted.current) setNotice(createdNotPlaced(node.title));
      } catch (error) {
        // Session perdue : la connexion est demandée par-dessus la page, rien d'autre à dire.
        if (isSessionLost(error)) return;
        if (mounted.current) setNotice(createRefused(title, messageOf(error)));
        context.onRefused?.();
      } finally {
        creating.current = false;
      }
    },
    [editor, context],
  );

  const pick = useCallback(
    (item: MentionItem) => {
      if ("create" in item) {
        void create(item.create);
        return;
      }
      setNotice(null);
      const { mention } = item;
      // Le titre du moment, si la page a été renommée depuis que le menu l'a listée.
      const label = mention.kind === "page" ? (pageTitle(mention.id) ?? mention.label) : mention.label;
      insertMention(editor, { ...mention, label });
    },
    [editor, create],
  );

  // Échap ferme le message, où que soit le focus.
  useEffect(() => {
    if (notice === null) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setNotice(null);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [notice]);

  const shouldOpen = useCallback((tr: Transaction) => outsideCode(tr) && !mentionMenuOpen(editor), [editor]);

  const menus = useMemo(
    () => (
      <>
        <SuggestionMenuController
          triggerCharacter={AT}
          shouldOpen={shouldOpen}
          getItems={getAtItems}
          suggestionMenuComponent={AtMenu}
          onItemClick={pick}
        />
        <SuggestionMenuController
          triggerCharacter={BRACKETS}
          shouldOpen={shouldOpen}
          getItems={getBracketItems}
          suggestionMenuComponent={BracketMenu}
          onItemClick={pick}
        />
      </>
    ),
    [getAtItems, getBracketItems, pick, shouldOpen],
  );

  return (
    <>
      {menus}
      {notice !== null && (
        <div className="mm-mention-notice" role="alert">
          <p dir="auto">{notice}</p>
          <button
            type="button"
            className="mm-icon-button"
            aria-label="Fermer le message"
            onClick={() => {
              setNotice(null);
              // Le bouton part avec le message : le focus revient au texte, pas à `body`.
              focusEditor(editor);
            }}
          >
            <CloseIcon />
          </button>
        </div>
      )}
    </>
  );
}

const AtMenu = (props: SuggestionMenuProps<MentionItem>) => <MentionMenu {...props} label="Mentionner" />;
const BracketMenu = (props: SuggestionMenuProps<MentionItem>) => <MentionMenu {...props} label="Lier une page" />;

/**
 * La liste d'un menu : un `listbox` nommé, ses lignes rangées par groupe. Le focus reste dans le
 * texte ; la ligne active est celle que BlockNote tient par les flèches (`aria-activedescendant` du
 * texte la nomme par son id) ; un clic choisit sans le prendre (la fenêtre de BlockNote retient le
 * `mousedown`). Les lignes ne changent qu'à la frappe : rien ne bouge sous le pointeur.
 */
function MentionMenu({ items, loadingState, selectedIndex, onItemClick, label }: SuggestionMenuProps<MentionItem> & { label: string }) {
  const active = useRef<HTMLLIElement | null>(null);
  useEffect(() => {
    active.current?.scrollIntoView?.({ block: "nearest" });
  }, [selectedIndex]);

  const groups: { name: string; rows: { item: MentionItem; index: number }[] }[] = [];
  items.forEach((item, index) => {
    const last = groups[groups.length - 1];
    if (last !== undefined && last.name === item.group) last.rows.push({ item, index });
    else groups.push({ name: item.group, rows: [{ item, index }] });
  });

  return (
    <div id="bn-suggestion-menu" className="mm-mention-menu" role="listbox" aria-label={label}>
      {groups.map((group, g) => (
        <ul key={group.name} className="mm-mention-group" role="group" aria-labelledby={`mm-mention-group-${g}`}>
          <li id={`mm-mention-group-${g}`} className="mm-mention-group-name" role="presentation">
            {group.name}
          </li>
          {group.rows.map(({ item, index }) => (
            <li
              key={item.key}
              ref={index === selectedIndex ? active : undefined}
              id={`bn-suggestion-menu-item-${index}`}
              className="mm-mention-option"
              role="option"
              aria-selected={index === selectedIndex}
              onClick={firstClick(() => onItemClick?.(item))}
            >
              {"create" in item ? <PlusIcon /> : item.group === GROUP_PAGES ? <PageIcon /> : null}
              <span className="mm-mention-option-title" dir="auto">
                {item.title}
              </span>
              {item.detail !== null && <span className="mm-mention-option-detail">{item.detail}</span>}
            </li>
          ))}
        </ul>
      ))}
      {items.length === 0 && (
        <p className="mm-mention-empty" role="status">
          {loadingState === "loaded" ? "Rien à proposer pour ce texte." : "Recherche…"}
        </p>
      )}
    </div>
  );
}
