// Les menus `@` et `[[` de l'éditeur (Task C4, R251, R618 à R621) : ceux de BlockNote
// (`SuggestionMenuController`, qui ouvre le menu à la frappe du déclencheur, quel que soit le clavier,
// suit ce qu'on tape, et tient les flèches, Entrée et Échap), avec nos lignes et notre liste.
//
// - `@` propose des pages (celles de l'arbre du compte, `findPages` : aucune autre ne peut y être),
//   des personnes (`loadPeople` : les membres de l'espace pour un membre, les comptes qui ont la page
//   ouverte pour un invité ; lues à l'ouverture du menu, jamais avant) et une date (`suggestDates`) ;
//   huit lignes au plus par groupe.
// - `[[` propose des pages, et, en dernier, « Créer la page « … » » : une sous-page de la page ouverte,
//   créée une fois par geste, puis mentionnée là où l'on avait tapé.
//
// Ce qui est posé : une mention (`kind`, `id`, et le libellé copié : le titre de la page à cet
// instant, le nom, la date), suivie d'une espace. Choisir retire du texte le déclencheur et ce qu'on
// avait tapé après lui ; Échap ferme le menu et laisse ce texte. Les menus restent fermés dans un
// bloc de code. Une mention ne prévient personne et ne donne aucun droit.
//
// Choisir « Créer la page » retire aussi du texte `[[` et le titre : tant que la mention n'est pas
// posée, un message de la page nomme ce titre (`notice.tsx`, R619 revu). Un refus du serveur s'y dit
// tel quel, et la page est relue ; une session perdue fait attendre le compte, puis redemander ; une
// page créée dont la place n'est plus là (ou dont l'éditeur est parti) y est dite, sans être liée.
//
// Ce que les menus savent de la page (`MentionContext`) est lu dans une référence : BlockNote relit
// les lignes dès que `getItems` change d'identité, sous la ligne active, tenue par son rang (R618).
import type { Role } from "@mementum/protocol";
import { trackPosition } from "@blocknote/core";
import { SuggestionMenu } from "@blocknote/core/extensions";
import { SuggestionMenuController, type SuggestionMenuProps } from "@blocknote/react";
import type { Transaction } from "prosemirror-state";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { createPage, isSessionLost } from "../api";
import type { RosterUser } from "../editor/collab";
import type { MementumEditor } from "../editor/debug";
import { PageIcon, PlusIcon } from "../icons";
import { messageOf } from "../session/forms";
import { findPages, pageTitle } from "../space/pageDirectory";
import { firstClick } from "../ui/firstClick";
import { dateId, dateLabel, suggestDates } from "./dates";
import { postNotice, replaceNotice } from "./notice";
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
  /** Tenue quand le compte revient après une session perdue (`useAfterLogin`) : la création repart. */
  afterLogin?: () => Promise<void>;
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

/**
 * Ce que disent un refus de création, une création qui attend le compte, et une page créée dont la
 * mention n'a pas trouvé sa place. Chacun nomme le titre.
 */
export const createRefused = (title: string, message: string) => `La page « ${title} » n'a pas pu être créée. ${message}`;
export const createWaits = (title: string) =>
  `La page « ${title} » n'est pas encore créée : ta session a pris fin. Elle sera créée et liée ici dès que tu te seras reconnecté.`;
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

type PmNode = Transaction["doc"];
type ResolvedPos = ReturnType<PmNode["resolve"]>;

/** Un texte s'écrit-il dans `parent`, et une mention y tient-elle ? */
function takesMentions(parent: PmNode): boolean {
  return parent.inlineContent && !parent.type.spec.code;
}

/** L'id du bloc qui porte la place `$pos`, ou `null`. */
function blockIdAt($pos: ResolvedPos): string | null {
  for (let depth = $pos.depth; depth > 0; depth -= 1) {
    const node = $pos.node(depth);
    if (node.type.name === "blockContainer") return String(node.attrs.id);
  }
  return null;
}

/** La fin du texte du bloc `id` de `doc`, si un texte s'y écrit ; sinon `null`. */
function endOfBlock(doc: PmNode, id: string): number | null {
  let end: number | null = null;
  doc.descendants((node, pos) => {
    if (end !== null) return false;
    if (node.type.name !== "blockContainer" || node.attrs.id !== id) return true;
    const content = node.firstChild;
    if (content !== null && takesMentions(content)) end = pos + 2 + content.content.size;
    return false;
  });
  return end;
}

/** Une place retenue dans le document : celle que suit BlockNote, et le bloc où elle était. */
type Place = { at: () => number; blockId: string | null };

/** Retient la place du curseur de `editor`, le temps d'un aller-retour au serveur. */
function holdPlace(editor: MementumEditor): Place {
  const { selection } = editor.prosemirrorState;
  // À gauche : ce qu'on tape en attendant vient après la mention.
  return { at: trackPosition(editor, selection.from, "left"), blockId: blockIdAt(selection.$from) };
}

/**
 * Pose `mention`, suivie d'une espace, à la place retenue `place`, si elle est encore dans son bloc
 * et qu'un texte s'y écrit ; à défaut (le bloc a changé de type : son contenu a été recréé, et la
 * place suivie n'y est plus), à la fin du texte de ce bloc, retrouvé par son id. Rend faux si rien
 * n'a pu être posé : le bloc est parti, c'est un bloc de code, ou l'éditeur n'est plus là ou plus
 * modifiable.
 */
function insertMentionAt(editor: MementumEditor, place: Place, mention: MentionProps): boolean {
  try {
    // Un éditeur retiré de la page (session recréée, autre page ouverte) n'écrit plus pour personne.
    if (!editor.isEditable || editor.domElement?.isConnected !== true) return false;
    let held: number | null = null;
    try {
      held = place.at();
    } catch {
      // La place suivie ne tient plus dans le document.
    }
    return editor.transact((tr) => {
      const node = tr.doc.type.schema.nodes.mention;
      if (node === undefined) return false;
      let pos: number | null = null;
      try {
        const $held = held === null ? null : tr.doc.resolve(held);
        if ($held !== null && takesMentions($held.parent) && blockIdAt($held) === place.blockId) pos = $held.pos;
      } catch {
        // Elle est hors du document.
      }
      if (pos === null && place.blockId !== null) pos = endOfBlock(tr.doc, place.blockId);
      if (pos === null) return false;
      tr.insert(pos, [node.create(mention), tr.doc.type.schema.text(" ")]);
      return true;
    });
  } catch {
    return false;
  }
}

/**
 * Crée la sous-page `title` de la page de `context`, puis pose sa mention à `place` dans `editor`.
 * Tant que la mention n'est pas posée, un message nomme le titre (`notice.tsx`) : il ne dépend ni de
 * l'éditeur ni de ses menus, que l'aller-retour peut voir partir.
 */
async function createAndMention(editor: MementumEditor, context: MentionContext, title: string, place: Place): Promise<void> {
  const ask = () => createPage({ parentId: context.pageId, title });
  /** Le message d'attente du compte, s'il est posé. */
  let waiting: number | null = null;
  try {
    let node;
    try {
      node = await ask();
    } catch (error) {
      if (!isSessionLost(error)) throw error;
      // Le `401` n'a rien créé : la connexion est demandée par-dessus la page, et la création
      // repart au retour du compte. D'ici là, et s'il ne revient pas, le message garde le titre.
      waiting = postNotice(createWaits(title));
      await (context.afterLogin?.() ?? new Promise<never>(() => {}));
      node = await ask();
    }
    const placed = insertMentionAt(editor, place, { kind: "page", id: node.id, label: node.title });
    const said = placed ? null : createdNotPlaced(node.title);
    if (waiting !== null) replaceNotice(waiting, said);
    else if (said !== null) postNotice(said);
  } catch (error) {
    const said = createRefused(title, messageOf(error));
    if (waiting !== null) replaceNotice(waiting, said);
    else postNotice(said);
    // Un droit, ou la page, ont changé : elle est à relire. Pas pour une session perdue.
    if (!isSessionLost(error)) context.onRefused?.();
  }
}

type Props = { editor: MementumEditor; context: MentionContext };

/** Les menus `@` et `[[` de l'éditeur `editor`. */
export function MentionMenus({ editor, context }: Props) {
  // Le contexte du moment, lu à chaque demande : `getItems` garde son identité quand il change
  // (un rôle relu), sinon BlockNote relirait les lignes sous la ligne active.
  const latest = useRef(context);
  useEffect(() => {
    latest.current = context;
  }, [context]);

  const getAtItems = useCallback((query: string) => atItems(query, latest.current, new Date()), []);
  const getBracketItems = useCallback(async (query: string) => bracketItems(query, latest.current), []);

  const pick = useCallback(
    (item: MentionItem) => {
      if ("create" in item) {
        // BlockNote vient de retirer `[[` et le titre : le curseur est là où va la mention.
        void createAndMention(editor, latest.current, item.create, holdPlace(editor));
        return;
      }
      const { mention } = item;
      // Le titre du moment, si la page a été renommée depuis que le menu l'a listée.
      const label = mention.kind === "page" ? (pageTitle(mention.id) ?? mention.label) : mention.label;
      insertMention(editor, { ...mention, label });
    },
    [editor],
  );

  const shouldOpen = useCallback((tr: Transaction) => outsideCode(tr) && !mentionMenuOpen(editor), [editor]);

  return useMemo(
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
