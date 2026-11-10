// Une page ouverte : bandeaux (lecture seule, corbeille) si besoin, titre, puis l'éditeur en
// direct. Une page à la corbeille se lit encore, et ne s'écrit plus : son titre et son éditeur
// sont verrouillés, et son bandeau la restaure.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { isSessionLost } from "../api";
import { CollabSession } from "../editor/collab";
import { requestedFixture } from "../editor/fixtures";
import { LazyEditor } from "../editor/LazyEditor";
import { ReadOnlyBanner } from "../editor/ReadOnlyBanner";
import type { SchemaCheck } from "../editor/schemaGate";
import { TrashIcon } from "../icons";
import { messageOf } from "../session/forms";
import { colorOf, useSession } from "../session/session";
import { canEdit } from "../share/levels";
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
  /** Sort la page de la corbeille ; rejette avec l'erreur du serveur s'il refuse. */
  onRestore: (id: string) => Promise<void>;
};

export function PageView({
  node,
  readOnly,
  checkSchema,
  autoFocusTitle,
  onTitleFocused,
  onSaved,
  onRestore,
}: Props) {
  const [session, setSession] = useState<CollabSession | null>(null);
  // `?fixture=<nom>`, en développement seulement : la fixture est chargée dans la page neuve.
  const [fixture] = useState(requestedFixture);
  // La présence porte le nom du compte et sa couleur.
  const { id: userId, name: userName } = useSession().me.user;
  const trashed = node.trashedAt !== null;
  /** Rien ne s'écrit : schéma d'un autre serveur, ou page à la corbeille. */
  const locked = readOnly || trashed;

  // Passer en lecture seule, ou en revenir (page restaurée), recrée la session, sur des
  // `Y.Doc` neufs.
  useEffect(() => {
    const opened = new CollabSession(
      node.id,
      locked ? { readOnly: true } : { readOnly: false, check: checkSchema },
      { name: userName, color: colorOf(userId) },
    );
    setSession(opened);
    return () => {
      opened.destroy();
      setSession((current) => (current === opened ? null : current));
    };
  }, [node.id, locked, checkSchema, userId, userName]);

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">{node.title}</span>
        {session && <SyncState session={session} />}
      </header>
      <article className="mm-page">
        {readOnly && <ReadOnlyBanner />}
        {trashed && (
          <TrashBanner
            // Restaurer demande de pouvoir modifier la page (et un onglet qui écrit encore).
            restore={!readOnly && canEdit(node.level) ? () => onRestore(node.id) : null}
          />
        )}
        <TitleInput
          node={node}
          autoFocus={autoFocusTitle}
          onAutoFocused={onTitleFocused}
          readOnly={locked}
          onSaved={onSaved}
        />
        {/* La session se connecte pendant que le code de l'éditeur se charge. */}
        {session && <LazyEditor session={session} fixture={fixture} />}
      </article>
    </>
  );
}

/**
 * Bandeau d'une page à la corbeille, avec « Restaurer » pour qui le peut. Le serveur refuse
 * quand c'est une page parente qui y a été mise (`409 parent_trashed`) : son message dit que
 * c'est elle qu'il faut restaurer, et s'affiche tel quel.
 */
function TrashBanner({ restore }: { restore: (() => Promise<void>) | null }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onClick = async () => {
    if (!restore || busy) return;
    setBusy(true);
    setError(null);
    try {
      await restore();
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; le bandeau attend.
      if (!isSessionLost(e)) setError(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mm-banner mm-banner-trash" role="status">
      <TrashIcon />
      <div className="mm-banner-text">
        <p>Cette page est à la corbeille.</p>
        {error && (
          <p className="mm-banner-error" role="alert">
            {error}
          </p>
        )}
      </div>
      {restore && (
        <button
          type="button"
          className="mm-button"
          aria-disabled={busy || undefined}
          onClick={() => void onClick()}
        >
          Restaurer
        </button>
      )}
    </div>
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
