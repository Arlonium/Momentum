// Une page ouverte : bandeau de lecture seule si besoin, titre, puis l'éditeur en direct.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { CollabSession } from "../editor/collab";
import { Editor } from "../editor/Editor";
import { requestedFixture } from "../editor/fixtures";
import { ReadOnlyBanner } from "../editor/ReadOnlyBanner";
import type { SchemaCheck } from "../editor/schemaGate";
import { TitleInput } from "./TitleInput";

type Props = {
  node: PageNode;
  /**
   * Le serveur n'a pas le schéma de ce client (lu au chargement, puis relu avant chaque
   * connexion, R35) : lecture seule pour de bon.
   */
  readOnly: boolean;
  /** Relit la version de schéma du serveur ; `false` met l'application en lecture seule. */
  checkSchema: SchemaCheck;
  autoFocusTitle: boolean;
  onTitleFocused: () => void;
  onSaved: (node: PageNode) => void;
};

export function PageView({
  node,
  readOnly,
  checkSchema,
  autoFocusTitle,
  onTitleFocused,
  onSaved,
}: Props) {
  const [session, setSession] = useState<CollabSession | null>(null);
  // `?fixture=<nom>`, en développement seulement : la fixture est chargée dans la page neuve.
  const [fixture] = useState(requestedFixture);

  // Passer en lecture seule recrée la session, sur des `Y.Doc` neufs.
  useEffect(() => {
    const opened = new CollabSession(
      node.id,
      readOnly ? { readOnly: true } : { readOnly: false, check: checkSchema },
    );
    setSession(opened);
    return () => {
      opened.destroy();
      setSession((current) => (current === opened ? null : current));
    };
  }, [node.id, readOnly, checkSchema]);

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">{node.title}</span>
        {session && <SyncState session={session} />}
      </header>
      <article className="mm-page">
        {readOnly && <ReadOnlyBanner />}
        <TitleInput
          node={node}
          autoFocus={autoFocusTitle}
          onAutoFocused={onTitleFocused}
          readOnly={readOnly}
          onSaved={onSaved}
        />
        {session && <Editor session={session} fixture={fixture} />}
      </article>
    </>
  );
}

/** État de la connexion, quand il mérite d'être dit. */
function SyncState({ session }: { session: CollabSession }) {
  const subscribe = useCallback((onChange: () => void) => session.subscribe(onChange), [session]);
  const state = useSyncExternalStore(subscribe, () => `${session.status}:${session.synced}`);
  const [status, synced] = state.split(":");
  let text = "";
  if (status === "disconnected") {
    // En lecture seule, rien ne partira au retour du réseau.
    text =
      synced === "true" && !session.readOnly
        ? "Hors ligne : tes modifications partiront au retour du réseau"
        : "Hors ligne";
  } else if (synced !== "true") {
    text = "Connexion…";
  }
  return (
    <span className="mm-topbar-status" role="status">
      {text}
    </span>
  );
}
