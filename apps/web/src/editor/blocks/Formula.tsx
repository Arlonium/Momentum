// Une formule affichée dans la page : en bloc (une équation) ou en ligne. Rien sur le chemin de
// l'ouverture d'une page (R423) : une formule n'est mise en forme que quand elle est à l'écran (ou
// près), et les formules à mettre en forme le sont l'une après l'autre, quelques millisecondes par
// image, jamais toutes d'un coup ; d'ici là, son texte source se montre à sa place. Cinquante
// formules de 10 000 caractères figeaient l'ouverture d'une page 22 s.
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

/** Temps de mise en forme par image, au plus (ms) : au-delà, la suite attend l'image suivante. */
const FRAME_BUDGET_MS = 8;

/** Le temps, remplaçable dans un test. */
let now = () => performance.now();
export function setClockForTests(clock: (() => number) | null): void {
  now = clock ?? (() => performance.now());
}

type Job = { run: () => void; cancelled: boolean };
const queue: Job[] = [];
let scheduled = false;

const nextFrame = (work: () => void) =>
  typeof requestAnimationFrame === "function" ? requestAnimationFrame(work) : setTimeout(work, 16);

function drain(): void {
  scheduled = false;
  const start = now();
  // Au moins une par image, puis tant que le budget de l'image tient.
  do {
    const job = queue.shift();
    if (job === undefined) return;
    if (!job.cancelled) job.run();
  } while (queue.length > 0 && now() - start < FRAME_BUDGET_MS);
  if (queue.length > 0) schedule();
}

function schedule(): void {
  if (scheduled) return;
  scheduled = true;
  nextFrame(drain);
}

/** Met `run` dans la file des mises en forme ; rend de quoi l'en retirer. */
function enqueue(run: () => void): () => void {
  const job: Job = { run, cancelled: false };
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

type Props = {
  latex: string;
  display: boolean;
  /** Tout de suite, sans attendre l'écran ni la file (l'aperçu d'un champ, à l'écran par nature). */
  eager?: boolean;
};

export function Formula({ latex, display, eager = false }: Props) {
  const ref = useRef<HTMLSpanElement>(null);
  const [outcome, setOutcome] = useState<{ latex: string; display: boolean; value: TexOutcome } | null>(null);
  const [depth, setDepth] = useState<number | null>(null);
  const current = outcome !== null && outcome.latex === latex && outcome.display === display ? outcome.value : null;

  // La profondeur de la page autour de la formule, mesurée avant d'attacher quoi que ce soit.
  useLayoutEffect(() => {
    if (ref.current !== null) setDepth(depthOf(ref.current));
  }, []);

  useLayoutEffect(() => {
    if (current !== null || ref.current === null) return;
    const set = () => setOutcome({ latex, display, value: typeset(latex, display) });
    // Déjà mise en forme, ou demandée tout de suite : rien à étaler.
    if (eager || isTypeset(latex, display)) {
      set();
      return;
    }
    let cancel = () => {};
    const stopWatching = whenShown(ref.current, () => {
      cancel = enqueue(set);
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
  if (current === null || depth === null) {
    return (
      <span ref={ref} className="mm-tex-pending" aria-busy="true">
        {source}
      </span>
    );
  }
  if (current.kind === "empty") return <span ref={ref} className="mm-tex-empty" />;
  if (current.kind === "formula" && current.depth <= depthAllowance(depth)) {
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
