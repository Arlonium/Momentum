// Les blocs commentés dans la page. Une pastille dans la marge droite de chaque bloc qui a un fil
// ouvert : un clic ouvre le panneau des commentaires sur ce fil. Et ce que le panneau doit savoir
// de la page pour ranger les fils : quels blocs commentés y sont, et dans quel ordre (R134).
//
// Rien de cela n'entre dans le document : les pastilles sont de l'interface posée par-dessus
// l'éditeur, dans son cadre, et les blocs sont lus dans ce qu'il affiche (le document de la
// session, en lecture seule comme en écriture). Aucune marque, aucun nœud : y-prosemirror
// supprimerait chez tout le monde ce qu'il ne sait pas construire.
//
// Une page peut avoir dix mille blocs : le suivi (`BlockTracker`) ne regarde que les blocs qui
// ont un fil. Il garde l'élément de chacun ; à chaque changement du document, il vérifie que cet
// élément est toujours là, sans parcourir la page, et ne cherche de nouveau que celui d'un bloc
// que ProseMirror a recréé (déplacé, retapé). Un bloc introuvable (supprimé) n'est recherché
// qu'une fois la frappe calmée (`RECHECK_MS`) : il peut revenir, par une annulation ou par
// l'écriture d'un autre. Les pastilles se replacent au rythme de l'affichage, une fois par image
// au plus.
//
// Pour qui peut commenter sans pouvoir modifier, l'éditeur est en lecture seule, et BlockNote n'y
// montre pas la poignée des blocs (ni son menu, ni « Commenter ») : un bouton « Commenter ce
// bloc » suit alors le bloc survolé, dans la même marge, et Ctrl+Alt+M commente le bloc où est
// la sélection du document (R176).
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { CommentIcon } from "../icons";
import { BLOCK, blockElement, blockOfSelection, blockText, isolate, shorten, shown } from "./dom";
import { isCommentShortcut } from "./shortcut";

/** Plus de blocs que cela à chercher : un seul parcours de la page plutôt qu'une recherche par bloc. */
const FULL_SCAN_AT = 8;
/** Un bloc introuvable est recherché cette durée après le dernier changement du document. */
export const RECHECK_MS = 1_000;
/** Durée de la mise en évidence d'un bloc qu'un fil fait voir. */
const GLOW_MS = 1_600;
/** Le début du texte d'un bloc qui nomme sa pastille : au plus tant de caractères. */
const LABEL_CHARS = 40;

/** Où sont, dans l'éditeur, les blocs commentés : ceux-là seulement. */
export class BlockTracker {
  private readonly elements = new Map<string, Element>();
  /** Les blocs cherchés sans succès : ils ne le sont de nouveau que sur demande. */
  private readonly absent = new Set<string>();
  /** L'éditeur affiché, sous lequel les blocs sont cherchés ; `null` : pas encore là. */
  readonly root: () => Element | null;

  constructor(root: () => Element | null) {
    this.root = root;
  }

  /** Parmi `ids`, les blocs cherchés dans la page et introuvables. */
  missing(ids: readonly string[]): string[] {
    return ids.filter((id) => this.absent.has(id));
  }

  /** L'éditeur est affiché. */
  hasRoot(): boolean {
    return this.root() !== null;
  }

  /**
   * Les éléments des blocs `ids` qui sont dans la page, dans l'ordre de `ids`. Un élément gardé
   * qui y est encore sert tel quel ; celui d'un bloc recréé est cherché de nouveau ; un bloc déjà
   * cherché sans succès ne l'est de nouveau que si `recheck`.
   */
  locate(ids: readonly string[], recheck: boolean): Map<string, Element> {
    const wanted = new Set(ids);
    for (const id of [...this.elements.keys()]) if (!wanted.has(id)) this.elements.delete(id);
    for (const id of [...this.absent]) if (!wanted.has(id)) this.absent.delete(id);
    const root = this.root();
    if (root === null) return new Map();
    const missing: string[] = [];
    for (const id of wanted) {
      const kept = this.elements.get(id);
      if (kept?.isConnected && kept.getAttribute("data-id") === id && root.contains(kept)) continue;
      this.elements.delete(id);
      if (recheck || !this.absent.has(id)) missing.push(id);
    }
    if (missing.length > FULL_SCAN_AT) {
      const sought = new Set(missing);
      for (const element of root.querySelectorAll(`${BLOCK}[data-id]`)) {
        const id = element.getAttribute("data-id")!;
        if (sought.has(id) && !this.elements.has(id)) this.elements.set(id, element);
      }
    } else {
      for (const id of missing) {
        const element = blockElement(root, id);
        if (element) this.elements.set(id, element);
      }
    }
    for (const id of missing) {
      if (this.elements.has(id)) this.absent.delete(id);
      else this.absent.add(id);
    }
    const found = new Map<string, Element>();
    for (const id of ids) {
      const element = this.elements.get(id);
      if (element) found.set(id, element);
    }
    return found;
  }

  /** Les blocs trouvés, dans l'ordre du document (un bloc avant ses enfants). */
  static order(found: ReadonlyMap<string, Element>): string[] {
    return [...found]
      .sort(([, a], [, b]) =>
        a === b ? 0 : a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1,
      )
      .map(([id]) => id);
  }
}

/** Ce que les pastilles lisent de la session de synchro. */
export type MarksSession = {
  synced: boolean;
  subscribe(listener: () => void): () => void;
  doc: {
    on(event: "update", listener: () => void): unknown;
    off(event: "update", listener: () => void): unknown;
  };
};

type Props = {
  /** Le cadre de l'éditeur, où les pastilles se placent. */
  frame: RefObject<HTMLElement | null>;
  session: MarksSession | null;
  /** Les blocs de tous les fils, résolus compris : leur présence et leur ordre vont au panneau. */
  blockIds: readonly string[];
  /** Les fils ouverts de chaque bloc, du plus ancien au plus récent : une pastille par bloc. */
  marks: ReadonlyMap<string, readonly string[]>;
  /**
   * Les blocs commentés qui sont dans la page, dans l'ordre (`null` : pas encore lus), et ceux
   * qui y ont été cherchés sans succès (un bloc pas encore cherché n'est dans aucun des deux).
   */
  onOrder: (order: string[] | null, absent: string[]) => void;
  onOpenThread: (threadId: string) => void;
  /** Le bloc à mettre en évidence un instant ; `seq` change à chaque demande. */
  highlight: { blockId: string; seq: number } | null;
  /** Donné : « Commenter ce bloc » suit le bloc survolé (éditeur en lecture seule). */
  onComment: ((blockId: string) => void) | null;
};

type Placed = { blockId: string; top: number; threadId: string; label: string };

function samePlaced(a: Placed[], b: Placed[]): boolean {
  return (
    a.length === b.length &&
    a.every(
      (p, i) =>
        p.blockId === b[i].blockId &&
        p.top === b[i].top &&
        p.threadId === b[i].threadId &&
        p.label === b[i].label,
    )
  );
}

function sameList(a: string[] | null, b: string[] | null | undefined): boolean {
  if (a === null || b === null || b === undefined) return a === b;
  return a.length === b.length && a.every((id, i) => id === b[i]);
}

function markLabel(count: number, text: string): string {
  const threads = `${count} fil${count > 1 ? "s" : ""} de commentaires`;
  return text === ""
    ? `${threads} sur ce bloc`
    : `${threads} sur « ${isolate(shorten(text, LABEL_CHARS))} »`;
}

export function BlockMarks({
  frame,
  session,
  blockIds,
  marks,
  onOrder,
  onOpenThread,
  highlight,
  onComment,
}: Props) {
  const [placed, setPlaced] = useState<Placed[]>([]);
  const [hover, setHover] = useState<{ blockId: string; top: number } | null>(null);
  const [glow, setGlow] = useState<{ top: number; left: number; width: number; height: number } | null>(
    null,
  );
  // Les données du dernier rendu : les mesures, faites hors du rendu, lisent celles-là.
  const latest = useRef({ blockIds, marks, onOrder });
  latest.current = { blockIds, marks, onOrder };
  /** Demande une mesure à la prochaine image. */
  const remeasure = useRef(() => {});
  const tracker = useMemo(() => {
    let editor: Element | null = null;
    return new BlockTracker(() => {
      const root = frame.current;
      if (root === null) return null;
      if (editor === null || !editor.isConnected || !root.contains(editor)) {
        editor = root.querySelector(".bn-editor");
      }
      return editor;
    });
  }, [frame]);

  useEffect(() => {
    if (session === null) return;
    let request: number | null = null;
    /** Les blocs introuvables sont à chercher de nouveau. */
    let recheck = true;
    let recheckTimer: ReturnType<typeof setTimeout> | undefined;
    let told: { order: string[] | null; absent: string[] } | undefined;
    const tell = (order: string[] | null, absent: string[]) => {
      if (told && sameList(order, told.order) && sameList(absent, told.absent)) return;
      told = { order, absent };
      latest.current.onOrder(order, absent);
    };
    /** Les éléments de la dernière mise en ordre, et cet ordre : ils ne bougent pas à la frappe. */
    let sorted = new Map<string, Element>();
    let order: string[] = [];

    const measure = () => {
      request = null;
      const root = frame.current;
      if (!session.synced || root === null || !tracker.hasRoot()) {
        tell(null, []);
        setPlaced((current) => (current.length === 0 ? current : []));
        return;
      }
      const { blockIds, marks } = latest.current;
      const found = tracker.locate(blockIds, recheck);
      recheck = false;
      // Remis en ordre seulement quand un élément a changé (bloc recréé, ajouté, parti) : la
      // comparaison de deux blocs voisins peut parcourir leurs frères.
      const same =
        found.size === sorted.size && [...found].every(([id, element]) => sorted.get(id) === element);
      if (!same) {
        sorted = found;
        order = BlockTracker.order(found);
      }
      tell(order, tracker.missing(blockIds));
      // Un bloc caché (enfant d'un bloc repliable fermé) n'a pas de pastille : sa boîte ne dit rien
      // de sa place. Son fil reste dans le panneau.
      const marked = order.filter(
        (id) => (marks.get(id)?.length ?? 0) > 0 && shown(found.get(id)!),
      );
      // Aucune pastille : rien à mesurer (pas de mise en page forcée à l'ouverture de la page).
      const origin = marked.length > 0 ? root.getBoundingClientRect().top : 0;
      const next = marked.map((blockId) => {
        const element = found.get(blockId)!;
        const threads = marks.get(blockId)!;
        return {
          blockId,
          top: Math.round(element.getBoundingClientRect().top - origin),
          threadId: threads[0],
          label: markLabel(threads.length, blockText(element)),
        };
      });
      setPlaced((current) => (samePlaced(current, next) ? current : next));
    };
    const schedule = () => {
      request ??= requestAnimationFrame(measure);
    };
    const onUpdate = () => {
      schedule();
      // Un bloc introuvable a pu revenir : il est cherché une fois la frappe calmée.
      clearTimeout(recheckTimer);
      recheckTimer = setTimeout(() => {
        recheck = true;
        schedule();
      }, RECHECK_MS);
    };
    session.doc.on("update", onUpdate);
    const offSession = session.subscribe(schedule);
    // La mise en page change sans que le document change : largeur de la fenêtre, image chargée,
    // bloc replié.
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
    if (frame.current) observer?.observe(frame.current);
    remeasure.current = schedule;
    schedule();
    return () => {
      if (request !== null) cancelAnimationFrame(request);
      clearTimeout(recheckTimer);
      session.doc.off("update", onUpdate);
      offSession();
      observer?.disconnect();
      remeasure.current = () => {};
    };
  }, [session, frame, tracker]);

  // Les fils ont changé : d'autres blocs à suivre, d'autres pastilles.
  useEffect(() => remeasure.current(), [blockIds, marks]);

  // Le bloc survolé, pour « Commenter ce bloc ». La marge et le bouton lui-même ne sont pas des
  // blocs : le survol reste au dernier bloc, jusqu'à ce que la souris quitte l'éditeur.
  useEffect(() => {
    const root = frame.current;
    if (onComment === null || root === null) {
      setHover(null);
      return;
    }
    const onMove = (event: MouseEvent) => {
      const element = (event.target as Element | null)?.closest?.(`${BLOCK}[data-id]`);
      if (!element || !root.contains(element)) return;
      const blockId = element.getAttribute("data-id")!;
      const top = Math.round(element.getBoundingClientRect().top - root.getBoundingClientRect().top);
      setHover((current) =>
        current?.blockId === blockId && current.top === top ? current : { blockId, top },
      );
    };
    const onLeave = () => setHover(null);
    // Au clavier : Ctrl+Alt+M commente le bloc où est la sélection du document (à la souris, ou
    // par la navigation au curseur du navigateur, F7). Rien ne s'écrit dans le document.
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || !isCommentShortcut(event)) return;
      const editor = tracker.root();
      const blockId = editor === null ? null : blockOfSelection(editor, document.getSelection());
      if (blockId === null) return;
      event.preventDefault();
      onComment(blockId);
    };
    root.addEventListener("mousemove", onMove);
    root.addEventListener("mouseleave", onLeave);
    document.addEventListener("keydown", onKey);
    return () => {
      root.removeEventListener("mousemove", onMove);
      root.removeEventListener("mouseleave", onLeave);
      document.removeEventListener("keydown", onKey);
    };
  }, [onComment, frame, tracker]);

  // Le bloc qu'un fil fait voir : un fond qui s'efface, posé par-dessus, hors du document.
  useEffect(() => {
    const root = frame.current;
    if (highlight === null || root === null) return;
    const element = blockElement(root, highlight.blockId);
    // Un bloc caché (dans un bloc repliable fermé) n'a pas de boîte à mettre en évidence ; celle
    // d'un bloc montré avant lui ne reste pas.
    if (element === null || !shown(element)) {
      setGlow(null);
      return;
    }
    const block = element.getBoundingClientRect();
    const origin = root.getBoundingClientRect();
    setGlow({
      top: block.top - origin.top,
      left: block.left - origin.left,
      width: block.width,
      height: block.height,
    });
    const timer = setTimeout(() => setGlow(null), GLOW_MS);
    return () => clearTimeout(timer);
  }, [highlight, frame]);

  const commentable = hover !== null && onComment !== null && !marks.has(hover.blockId);

  return (
    <>
      {glow && (
        <div
          className="mm-block-glow"
          aria-hidden="true"
          style={{ top: glow.top, left: glow.left, width: glow.width, height: glow.height }}
        />
      )}
      {placed.length > 0 && (
        <ul className="mm-block-marks" aria-label="Blocs commentés">
          {placed.map((mark) => (
            <li key={mark.blockId} style={{ top: mark.top }}>
              <button
                type="button"
                className="mm-block-mark"
                aria-label={mark.label}
                title={mark.label}
                onClick={() => onOpenThread(mark.threadId)}
              >
                <CommentIcon size={14} />
                {(marks.get(mark.blockId)?.length ?? 0) > 1 && (
                  <span className="mm-block-mark-count" aria-hidden="true">
                    {marks.get(mark.blockId)!.length}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
      {commentable && (
        <button
          type="button"
          className="mm-block-mark mm-block-comment"
          style={{ top: hover.top }}
          aria-label="Commenter ce bloc"
          title="Commenter ce bloc"
          onClick={() => onComment(hover.blockId)}
        >
          <CommentIcon size={14} />
          <span aria-hidden="true">+</span>
        </button>
      )}
    </>
  );
}
