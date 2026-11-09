// Une page ouverte : bandeau de lecture seule si besoin, titre, puis l'éditeur en direct.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { CollabSession } from "../editor/collab";
import { Editor } from "../editor/Editor";
import { ReadOnlyBanner } from "../editor/ReadOnlyBanner";
import { CLIENT_SCHEMA_VERSION } from "../editor/schema";
import { TitleInput } from "./TitleInput";

type Props = {
  node: PageNode;
  /** `schemaVersion` de `GET /api/v1/info`, lu au chargement de l'application. */
  serverSchemaVersion: number;
  autoFocusTitle: boolean;
  onTitleFocused: () => void;
  onSaved: (node: PageNode) => void;
};

export function PageView({
  node,
  serverSchemaVersion,
  autoFocusTitle,
  onTitleFocused,
  onSaved,
}: Props) {
  const readOnly = serverSchemaVersion !== CLIENT_SCHEMA_VERSION;
  const [session, setSession] = useState<CollabSession | null>(null);

  useEffect(() => {
    const opened = new CollabSession(node.id, readOnly);
    setSession(opened);
    return () => {
      opened.destroy();
      setSession((current) => (current === opened ? null : current));
    };
  }, [node.id, readOnly]);

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
        {session && <Editor session={session} />}
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
    text = synced === "true" ? "Hors ligne : tes modifications partiront au retour du réseau" : "Hors ligne";
  } else if (synced !== "true") {
    text = "Connexion…";
  }
  return (
    <span className="mm-topbar-status" role="status">
      {text}
    </span>
  );
}
