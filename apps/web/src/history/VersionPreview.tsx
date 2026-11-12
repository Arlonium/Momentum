// L'aperçu d'une version dans la fenêtre « Historique » (R135) : dans le même éditeur que la page
// (`PreviewEditor`), en lecture seule.
//
// L'aperçu n'écrit jamais, nulle part. Il vit dans un `Y.Doc` neuf, sans fournisseur : aucune
// WebSocket, aucun `BroadcastChannel`, aucune présence. Jamais dans le document de la session de
// la page : y-prosemirror supprime d'un document ce qu'il ne sait pas construire, dès
// l'affichage, et une version peut porter des blocs d'un autre schéma ; ces suppressions restent
// dans le document de l'aperçu, que rien ne lit.
// - Une version : son état est lu au moment où on la choisit (`GET …/state`, plusieurs
//   mégaoctets pour une grande page), puis appliqué dans un document neuf.
// - La version actuelle : une copie figée du document de la page tel que le serveur le connaît
//   (`provider.doc` de la session), prise au moment où on la choisit, et reprise quand on la
//   choisit de nouveau (R192). Elle ne le suit pas : l'historique ne pose aucun écouteur sur un
//   document de la session. Une copie qui suivrait se mettrait à jour dans l'événement `update`
//   de ce document, et une exception de l'éditeur de l'aperçu remonterait alors dans la
//   transaction de la session, qui resterait bloquée : plus rien n'en partirait ni ne s'en
//   afficherait.
// Chaque document d'aperçu est détruit quand on le quitte et à la fermeture, et le
// téléchargement d'une version qu'on quitte avant son arrivée est annulé.
import { Component, useEffect, useRef, useState, type ReactNode } from "react";
import * as Y from "yjs";
import { ApiError, getVersionState } from "../api";
import { PreviewEditor } from "../editor/Editor";
import { WarningIcon } from "../icons";
import { messageOf } from "../session/forms";

type Props = {
  pageId: string;
  /** La version à montrer ; `null` : la page telle qu'elle est (« Version actuelle »). */
  versionId: string | null;
  /**
   * Le document de la page tel que le serveur le donne à la session (`provider.doc`), pour la
   * version actuelle ; `null` : pas de session ouverte en ce moment. Il n'est que lu.
   */
  current: Y.Doc | null;
  /** Change à chaque choix de la version actuelle : sa copie est reprise. */
  snapshot?: number;
  /** Le serveur ne trouve pas l'état de la version (`404`) : elle a pu sortir de l'historique. */
  onMissing?: () => void;
};

/** Ce que montre l'aperçu, rangé sous la demande qui l'a produit (`key`). */
type Shown =
  | { key: string; status: "loading" }
  | { key: string; status: "ready"; doc: Y.Doc }
  | { key: string; status: "failed"; message: string }
  | { key: string; status: "unreadable" };

/** Un document neuf, rempli par l'état d'une version ; `null` si Yjs ne sait pas le lire. */
export function versionDoc(state: Uint8Array): Y.Doc | null {
  const doc = new Y.Doc();
  try {
    Y.applyUpdate(doc, state);
    return doc;
  } catch {
    doc.destroy();
    return null;
  }
}

/**
 * Une copie figée de `source` : son état encodé, appliqué à un document neuf, hors de toute
 * transaction de `source`. Rien n'est posé sur `source`.
 */
export function frozenCopy(source: Y.Doc): Y.Doc {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, Y.encodeStateAsUpdate(source));
  return doc;
}

export function VersionPreview({ pageId, versionId, current, snapshot = 0, onMissing }: Props) {
  const [shown, setShown] = useState<Shown | null>(null);
  /** Change à chaque « Réessayer » : l'état est relu. */
  const [attempt, setAttempt] = useState(0);
  // Le rappel du dernier rendu : en changer ne relit rien.
  const missing = useRef(onMissing);
  missing.current = onMissing;
  /** La demande en cours ; `null` : la version actuelle, sans session pour la copier. */
  const key =
    versionId === null
      ? current === null
        ? null
        : `current:${current.guid}:${snapshot}`
      : `version:${pageId}:${versionId}:${attempt}`;

  // Une demande, et le document qu'elle crée. Quitter la demande (une autre version, la version
  // actuelle reprise, la fermeture) annule son téléchargement et détruit son document : l'éditeur
  // qui le montrait est déjà retiré, puisque l'aperçu ne montre que ce qui est rangé sous la
  // demande en cours.
  useEffect(() => {
    if (key === null) return;
    let made: Y.Doc | null = null;
    if (versionId === null) {
      made = frozenCopy(current!);
      setShown({ key, status: "ready", doc: made });
      return () => made?.destroy();
    }
    const download = new AbortController();
    setShown({ key, status: "loading" });
    getVersionState(pageId, versionId, download.signal).then(
      (state) => {
        if (download.signal.aborted) return;
        made = versionDoc(state);
        setShown(made ? { key, status: "ready", doc: made } : { key, status: "unreadable" });
      },
      (error: unknown) => {
        if (download.signal.aborted) return;
        setShown({ key, status: "failed", message: messageOf(error) });
        if (error instanceof ApiError && error.status === 404) missing.current?.();
      },
    );
    return () => {
      download.abort();
      made?.destroy();
    };
    // La demande dit tout : `pageId`, `versionId` et `current` y sont.
  }, [key]);

  // Une réponse rangée sous une autre demande ne vaut pas pour celle-ci.
  const state = shown !== null && shown.key === key ? shown : null;
  if (state === null || state.status === "loading") {
    return (
      <p className="mm-history-note">
        {versionId === null ? "Chargement…" : "Chargement de la version…"}
      </p>
    );
  }
  switch (state.status) {
    case "unreadable":
      return <Problem>Cette version n'a pas pu être lue.</Problem>;
    case "failed":
      return (
        <div className="mm-history-problem" role="alert">
          <WarningIcon />
          <p>{state.message}</p>
          <button type="button" className="mm-button mm-button-small" onClick={() => setAttempt((n) => n + 1)}>
            Réessayer
          </button>
        </div>
      );
    case "ready":
      return (
        <PreviewBoundary key={state.key}>
          <PreviewEditor doc={state.doc} />
        </PreviewBoundary>
      );
  }
}

function Problem({ children }: { children: ReactNode }) {
  return (
    <div className="mm-history-problem" role="status">
      <WarningIcon />
      <p>{children}</p>
    </div>
  );
}

/**
 * Une version que BlockNote n'arrive pas à afficher : un message à la place de l'aperçu, et le
 * reste de la fenêtre en place. L'erreur va dans la console, pour qui cherche la cause.
 */
class PreviewBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown): void {
    console.error("Mementum : l'aperçu d'une version n'a pas pu s'afficher.", error);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return <Problem>Cette version n'a pas pu s'afficher.</Problem>;
  }
}
