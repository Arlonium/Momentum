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
// - La version actuelle : une copie du document de la page tel que le serveur le connaît
//   (`provider.doc` de la session), qui suit ce qui y arrive. La copie lit ce document, elle n'y
//   écrit rien.
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
   * version actuelle ; `null` : pas de session ouverte en ce moment.
   */
  current: Y.Doc | null;
  /** Le serveur ne trouve pas l'état de la version (`404`) : elle a pu sortir de l'historique. */
  onMissing?: () => void;
};

/** L'état d'une version, lu pour `versionId`. */
type Loaded =
  | { versionId: string; status: "loading" }
  | { versionId: string; status: "ready"; doc: Y.Doc }
  | { versionId: string; status: "failed"; message: string }
  | { versionId: string; status: "unreadable" };

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
 * Une copie de `source` qui suit ce qui y arrive, sans jamais y écrire ; `null` sans source.
 * Une source neuve (la session de la page a été remplacée) donne une copie neuve.
 */
function useCopyOf(source: Y.Doc | null): Y.Doc | null {
  const [copy, setCopy] = useState<{ source: Y.Doc; doc: Y.Doc } | null>(null);
  useEffect(() => {
    if (source === null) return;
    const doc = new Y.Doc();
    Y.applyUpdate(doc, Y.encodeStateAsUpdate(source));
    const follow = (update: Uint8Array) => Y.applyUpdate(doc, update);
    source.on("update", follow);
    setCopy({ source, doc });
    return () => {
      source.off("update", follow);
    };
  }, [source]);
  return copy !== null && copy.source === source ? copy.doc : null;
}

export function VersionPreview({ pageId, versionId, current, onMissing }: Props) {
  const copy = useCopyOf(versionId === null ? current : null);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  /** Change à chaque « Réessayer » : l'état est relu. */
  const [attempt, setAttempt] = useState(0);
  // Le rappel du dernier rendu : en changer ne relit rien.
  const missing = useRef(onMissing);
  missing.current = onMissing;

  useEffect(() => {
    if (versionId === null) return;
    let stopped = false;
    setLoaded({ versionId, status: "loading" });
    getVersionState(pageId, versionId).then(
      (state) => {
        if (stopped) return;
        const doc = versionDoc(state);
        setLoaded(doc ? { versionId, status: "ready", doc } : { versionId, status: "unreadable" });
      },
      (error: unknown) => {
        if (stopped) return;
        setLoaded({ versionId, status: "failed", message: messageOf(error) });
        if (error instanceof ApiError && error.status === 404) missing.current?.();
      },
    );
    return () => {
      stopped = true;
    };
  }, [pageId, versionId, attempt]);

  if (versionId === null) {
    if (copy === null) return <p className="mm-history-note">Chargement…</p>;
    return (
      <PreviewBoundary key="current">
        <PreviewEditor doc={copy} />
      </PreviewBoundary>
    );
  }
  // Une réponse rangée sous une autre version ne vaut pas pour celle-ci.
  const state: Loaded =
    loaded !== null && loaded.versionId === versionId ? loaded : { versionId, status: "loading" };
  switch (state.status) {
    case "loading":
      return <p className="mm-history-note">Chargement de la version…</p>;
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
        <PreviewBoundary key={versionId}>
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
