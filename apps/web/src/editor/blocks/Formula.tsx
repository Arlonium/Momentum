// Une formule affichée dans la page : en bloc (une équation) ou en ligne. Rien sur le chemin de
// l'ouverture d'une page, ni du retour sur elle (R423, R526) : une formule n'est attachée que quand
// elle est à l'écran (ou près), déjà calculée ou non, et les formules à attacher le sont l'une après
// l'autre, dans une file, quelques milliers d'éléments par image, jamais toutes d'un coup ; d'ici là,
// son texte source se montre à sa place. Le coût d'une formule est sa mise en page autant que son
// calcul : cinquante fois la même formule, déjà calculée, figeaient l'onglet 7 s au retour sur la
// page (relecture de C2).
//
// Avant d'attacher le HTML de KaTeX, la profondeur de la page autour de la formule est mesurée, et
// la profondeur d'éléments de la formule comparée à ce qu'elle laisse (`depthAllowance`, R525) :
// trop profonde pour sa place, la formule se montre en texte, avec un message, et l'onglet ne plante
// pas. La profondeur se relit à chaque montage : un bloc déplacé ou imbriqué est recréé (R223).
//
// Une formule refusée ou invalide se montre en texte (`dir="ltr"`, jamais du HTML), avec ce qui
// l'empêche (`refusal`).
import { useLayoutEffect, useRef, useState } from "react";
import { depthAllowance, isTypeset, refusal, TOO_NESTED, typeset, type TexOutcome } from "./tex";

/**
 * Éléments attachés par image, au plus : au-delà, la suite attend l'image suivante. Une formule
 * plus grande s'attache seule dans son image (mesuré : registre, R526).
 */
export const FRAME_NODES = 5_000;

/** Temps de calcul par image, au plus (ms) : au-delà, la suite attend l'image suivante. */
const FRAME_BUDGET_MS = 8;

/** Le temps, remplaçable dans un test. */
let now = () => performance.now();
export function setClockForTests(clock: (() => number) | null): void {
  now = clock ?? (() => performance.now());
}

type Job = {
  /** Son élément dans la page : la file suit l'ordre du document. */
  element: Element;
  /** La formule, calculée ou relue dans le cache : ce qu'elle attachera, et si elle vient d'être calculée. */
  measure: () => { nodes: number; computed: boolean };
  attach: () => void;
  cancelled: boolean;
  /** Calculée à une image où le temps manquait : elle s'attache à la suivante. */
  deferred: boolean;
};
const queue: Job[] = [];
let scheduled = false;

/** Vide la file (entre deux tests : une image remplacée par un test ne vient jamais). */
export function resetQueueForTests(): void {
  queue.length = 0;
  scheduled = false;
}

/** Dans l'ordre du document : la formule du haut d'abord. */
const inDocumentOrder = (a: Job, b: Job) =>
  a.element === b.element ? 0 : a.element.compareDocumentPosition(b.element) & Node.DOCUMENT_POSITION_PRECEDING ? 1 : -1;

const nextFrame = (work: () => void) =>
  typeof requestAnimationFrame === "function" ? requestAnimationFrame(work) : setTimeout(work, 16);

function drain(): void {
  scheduled = false;
  const start = now();
  let attached = 0;
  let any = false;
  queue.sort(inDocumentOrder);
  while (queue.length > 0) {
    const job = queue[0]!;
    if (job.cancelled) {
      queue.shift();
      continue;
    }
    // Au moins une par image, puis tant que le temps et les éléments de l'image tiennent.
    if (any && now() - start >= FRAME_BUDGET_MS) break;
    const { nodes, computed } = job.measure();
    // Calculée à l'instant, au-delà du temps de l'image : sa mise en page attend l'image suivante,
    // pour ne pas s'ajouter au calcul.
    if (computed && !job.deferred && now() - start >= FRAME_BUDGET_MS) {
      job.deferred = true;
      break;
    }
    if (any && attached + nodes > FRAME_NODES) break;
    queue.shift();
    job.attach();
    attached += nodes;
    any = true;
  }
  if (queue.length > 0) schedule();
}

function schedule(): void {
  if (scheduled) return;
  scheduled = true;
  nextFrame(drain);
}

/** Met la formule dans la file des attaches ; rend de quoi l'en retirer. */
function enqueue(element: Element, measure: Job["measure"], attach: () => void): () => void {
  const job: Job = { element, measure, attach, cancelled: false, deferred: false };
  queue.push(job);
  schedule();
  return () => {
    job.cancelled = true;
  };
}

/** Les formules qui attendent d'être à l'écran, suivies par un seul observateur. */
const waiting = new Map<Element, () => void>();
let observer: IntersectionObserver | null = null;

/** Appelle `onShown` quand `element` est à l'écran ou près (400 px) ; tout de suite sans observateur. */
function whenShown(element: Element, onShown: () => void): () => void {
  if (typeof IntersectionObserver !== "function") {
    onShown();
    return () => {};
  }
  observer ??= new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        const shown = waiting.get(entry.target);
        waiting.delete(entry.target);
        observer?.unobserve(entry.target);
        shown?.();
      }
    },
    { rootMargin: "400px 0px" },
  );
  waiting.set(element, onShown);
  observer.observe(element);
  return () => {
    waiting.delete(element);
    observer?.unobserve(element);
  };
}

/** Le nombre d'éléments au-dessus de `element`, lui compris. */
function depthOf(element: Element): number {
  let depth = 0;
  for (let at: Element | null = element; at !== null; at = at.parentElement) depth += 1;
  return depth;
}

/** Ce qu'une formule attache : ses éléments, ou un seul pour un texte (refusée, invalide, vide). */
const nodesOf = (outcome: TexOutcome) => (outcome.kind === "formula" ? outcome.nodes : 1);

type Props = {
  latex: string;
  display: boolean;
  /** Tout de suite, sans attendre l'écran ni la file (l'aperçu d'un champ, à l'écran par nature). */
  eager?: boolean;
};

export function Formula({ latex, display, eager = false }: Props) {
  const ref = useRef<HTMLSpanElement>(null);
  const [outcome, setOutcome] = useState<{ latex: string; display: boolean; value: TexOutcome } | null>(null);
  const [pageDepth, setPageDepth] = useState<number | null>(null);
  const current = outcome !== null && outcome.latex === latex && outcome.display === display ? outcome.value : null;

  // La profondeur de la page autour de la formule, mesurée avant d'attacher quoi que ce soit.
  useLayoutEffect(() => {
    if (ref.current !== null) setPageDepth(depthOf(ref.current));
  }, []);

  useLayoutEffect(() => {
    if (current !== null || ref.current === null) return;
    if (eager) {
      setOutcome({ latex, display, value: typeset(latex, display) });
      return;
    }
    // Déjà calculée ou non : à l'écran, puis dans la file, comme les autres (R526).
    let value: TexOutcome | null = null;
    const measure = () => {
      const computed = !isTypeset(latex, display);
      value = typeset(latex, display);
      return { nodes: nodesOf(value), computed };
    };
    let cancel = () => {};
    const element = ref.current;
    const stopWatching = whenShown(element, () => {
      cancel = enqueue(element, measure, () => setOutcome({ latex, display, value: value ?? typeset(latex, display) }));
    });
    return () => {
      stopWatching();
      cancel();
    };
  }, [latex, display, eager, current]);

  const source = (
    <span className="mm-tex-source" dir="ltr">
      {latex}
    </span>
  );
  if (current === null || pageDepth === null) {
    return (
      <span ref={ref} className="mm-tex-pending" aria-busy="true">
        {source}
      </span>
    );
  }
  if (current.kind === "empty") return <span ref={ref} className="mm-tex-empty" />;
  if (current.kind === "formula" && current.depth <= depthAllowance(pageDepth)) {
    // KaTeX à `trust: false` échappe ce qu'il reçoit : son HTML n'a ni lien, ni image, ni attribut
    // venu de la formule.
    return <span ref={ref} className="mm-tex" dangerouslySetInnerHTML={{ __html: current.html }} />;
  }
  const why = current.kind === "formula" ? TOO_NESTED : refusal(current);
  return (
    <span ref={ref} className={current.kind === "invalid" ? "mm-tex-refused mm-tex-invalid" : "mm-tex-refused"}>
      {source}
      <span className="mm-tex-why">{why}</span>
    </span>
  );
}
