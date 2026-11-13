// Une page ouverte : barre du haut (titre, état de la connexion, présents, « Commentaires »,
// « Partager » pour qui a l'accès complet, puis le menu « … » et son « Historique »), bandeaux
// (lecture seule, corbeille) si besoin, titre, puis l'éditeur en direct ; à droite, le panneau
// des commentaires quand il est ouvert.
//
// Les commentaires (R134) n'écrivent rien dans le document : ils se lisent à tout niveau, et
// s'écrivent avec « Peut commenter », même quand l'éditeur est en lecture seule. Le panneau
// rend le focus, en se fermant, à ce qui l'avait avant qu'il s'ouvre.
//
// Rien ne s'écrit quand le schéma du serveur n'est plus celui de l'onglet (R35), quand la page
// est à la corbeille, ou quand le niveau du compte ne permet que de la lire : le titre et
// l'éditeur sont verrouillés, et un bandeau dit pourquoi. Ce que le compte peut faire de la
// page n'est pas lu qu'à son ouverture : la page est relue avant chaque connexion de synchro
// (`usePageGate`), que le serveur ferme quand ses droits changent. Un niveau qui baisse, ou une
// page jetée ailleurs, la font passer en lecture seule aussitôt, sur des `Y.Doc` neufs ; un
// niveau qui monte ne prend effet qu'à la prochaine ouverture (le bandeau a un bouton pour
// cela) : l'éditeur de qui lit n'est pas remplacé sous ses yeux. La première relecture fait
// encore partie de l'ouverture : la page s'ouvre avec le niveau que dit le serveur, pas avec
// celui d'une barre latérale qui daterait d'avant.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { isSessionLost } from "../api";
import { BlockMarks } from "../comments/BlockMarks";
import { CommentsButton } from "../comments/CommentsButton";
import { CommentsPanel } from "../comments/CommentsPanel";
import { useComments } from "../comments/useComments";
import { useCommentsPanel } from "../comments/useCommentsPanel";
import { CollabSession } from "../editor/collab";
import { requestedFixture } from "../editor/fixtures";
import { LazyEditor } from "../editor/LazyEditor";
import { ReaderBanner, ReadOnlyBanner } from "../editor/ReadOnlyBanner";
import type { SchemaCheck } from "../editor/schemaGate";
import { TrashIcon } from "../icons";
import { messageOf } from "../session/forms";
import { colorOf, useSession } from "../session/session";
import { useAfterLogin } from "../session/useAfterLogin";
import { canComment, canEdit, writesOutsideEditor } from "../share/levels";
import { ShareMenu } from "../share/ShareMenu";
import { PageMenu } from "./PageMenu";
import { Presence } from "./Presence";
import { TitleInput } from "./TitleInput";
import { usePageGate } from "./usePageGate";

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
  /** Ce que le serveur dit de la page, relue avant chaque connexion de synchro. */
  onReread: (node: PageNode) => void;
  /** Le serveur dit que la page n'est plus là pour ce compte. */
  onLost: (id: string) => void;
  /** Les accès de la page ont changé par le menu Partager : la barre latérale est à relire. */
  onShared: () => void;
  /** Le serveur a refusé un changement d'accès à qui n'a plus l'accès complet : son message. */
  onShareRefused: (message: string) => void;
};

export function PageView({
  node,
  readOnly,
  checkSchema,
  autoFocusTitle,
  onTitleFocused,
  onSaved,
  onRestore,
  onReread,
  onLost,
  onShared,
  onShareRefused,
}: Props) {
  const [session, setSession] = useState<CollabSession | null>(null);
  // `?fixture=<nom>`, en développement seulement : la fixture est chargée dans la page neuve.
  const [fixture] = useState(requestedFixture);
  // La présence porte le nom du compte et sa couleur.
  const { me } = useSession();
  const { id: userId, name: userName } = me.user;
  const afterLogin = useAfterLogin(me);
  const trashed = node.trashedAt !== null;
  const editable = canEdit(node.level);
  /**
   * La page est ouverte en lecture : son niveau ne permettait pas de la modifier, à
   * l'ouverture ou depuis. Elle le reste quand il remonte, jusqu'à ce qu'on la rouvre.
   */
  const [reader, setReader] = useState(!editable);
  if (!editable && !reader) setReader(true);
  /** Le serveur n'a pas encore dit, depuis l'ouverture, ce que le compte peut faire de la page. */
  const opening = useRef(true);
  const onPage = useCallback(
    (next: PageNode) => {
      const first = opening.current;
      opening.current = false;
      onReread(next);
      // Le niveau a monté avant l'ouverture, et la barre latérale ne le savait pas : la page
      // s'ouvre en écriture, personne ne la lisait encore. `onReread` passe d'abord : la règle
      // du dessus reposerait la lecture si elle voyait encore l'ancien niveau.
      if (first && canEdit(next.level)) setReader(false);
    },
    [onReread],
  );
  /** Rien ne s'écrit : schéma d'un autre serveur, page à la corbeille, ou page qu'on ne fait que lire. */
  const locked = readOnly || trashed || reader;

  const { id } = node;
  const onGone = useCallback(() => onLost(id), [onLost, id]);
  const { check, reread } = usePageGate({
    id,
    writable: !locked,
    // Commenter ou restaurer depuis une session en lecture seule : le schéma compte aussi (R177).
    // Déjà sous un autre schéma, rien de plus à relire.
    writesOutsideEditor: !readOnly && writesOutsideEditor(node.level, trashed),
    checkSchema,
    onPage,
    onGone,
    afterLogin,
  });

  // Le menu Partager n'a plus rien à montrer : la personne n'a plus l'accès complet (elle s'est
  // baissée ou retirée elle-même, ou on le lui a retiré). La page est relue, et suit les règles
  // de la lecture seule ou de l'accès perdu ; le refus qui l'a révélé, s'il y en a un, reste dit.
  const accessLost = useCallback(
    (refusal: string | null) => {
      if (refusal !== null) onShareRefused(refusal);
      void reread();
    },
    [onShareRefused, reread],
  );

  // Passer en lecture seule, ou en revenir (page restaurée, page rouverte), recrée la session,
  // sur des `Y.Doc` neufs.
  useEffect(() => {
    const opened = new CollabSession(
      id,
      { readOnly: locked, check },
      { name: userName, color: colorOf(userId) },
    );
    setSession(opened);
    return () => {
      opened.destroy();
      setSession((current) => (current === opened ? null : current));
    };
  }, [id, locked, check, userId, userName]);

  /** Relit la page : un refus du serveur dit qu'un droit, ou la page, ont changé entre-temps. */
  const rereadPage = useCallback(() => void reread(), [reread]);

  // Les fils de commentaires, relus à chaque trame `comments` de la session de synchro, et au
  // retour du compte après une session perdue. Un refus du serveur (un droit, ou la page, ont
  // changé entre-temps) fait aussi relire la page : son niveau et la corbeille décident des
  // boutons du panneau.
  const comments = useComments(id, session, { onRefused: rereadPage, afterLogin });
  /** Autour de l'éditeur : les pastilles s'y placent, les blocs y sont cherchés. */
  const frame = useRef<HTMLDivElement>(null);
  /** La page (bandeaux, titre, éditeur). */
  const article = useRef<HTMLElement>(null);
  /** Le menu « … » n'est plus là quand sa fenêtre se ferme : le focus va au titre de la page. */
  const focusTitle = useCallback(
    () => article.current?.querySelector<HTMLElement>(".mm-title-input")?.focus(),
    [],
  );
  const wiring = useCommentsPanel(comments, frame);
  /** Commenter d'ici : le niveau, hors corbeille, depuis un onglet du schéma du serveur (R177). */
  const commentsWritable = canComment(node.level) && !trashed && !readOnly;
  /**
   * Éditeur en lecture seule, où BlockNote ne montre pas la poignée des blocs : « Commenter ce
   * bloc » suit le bloc survolé (R176). Sinon, « Commenter » est dans le menu de la poignée.
   */
  const hoverComment = commentsWritable && locked ? wiring.commentBlock : null;
  const menuComment = commentsWritable && !locked ? wiring.commentBlock : null;
  // L'éditeur ne se rend pas de nouveau quand les fils, le panneau ou les pastilles changent.
  const editor = useMemo(
    () => session && <LazyEditor session={session} fixture={fixture} onComment={menuComment} />,
    [session, fixture, menuComment],
  );

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">{node.title}</span>
        {session && <SyncState session={session} />}
        {session && <Presence session={session} selfId={userId} />}
        <CommentsButton {...wiring.button} />
        {/* Les accès ne se voient et ne se changent qu'avec l'accès complet, et rien ne s'y
            change à la corbeille, ni depuis un onglet qui n'écrit plus (R35). */}
        {node.level === "full" && !trashed && !readOnly && (
          <ShareMenu node={node} onChanged={onShared} onAccessLost={accessLost} />
        )}
        {/* « Historique » (à partir de « Peut modifier ») et les autres actions. La fenêtre
            n'est chargée qu'à la demande ; un refus du serveur fait relire la page, comme pour
            les commentaires. */}
        <PageMenu
          node={node}
          session={session}
          readOnly={readOnly}
          selfId={userId}
          afterLogin={afterLogin}
          focusFallback={focusTitle}
          onRefused={rereadPage}
        />
      </header>
      <div className="mm-page-body">
        <article ref={article} className="mm-page">
          {readOnly && <ReadOnlyBanner />}
          {reader && !readOnly && !trashed && (
            <ReaderBanner reopen={editable ? () => setReader(false) : null} />
          )}
          {trashed && (
            <TrashBanner
              // Restaurer demande de pouvoir modifier la page (et un onglet qui écrit encore).
              restore={!readOnly && editable ? () => onRestore(id) : null}
            />
          )}
          <TitleInput
            node={node}
            autoFocus={autoFocusTitle}
            onAutoFocused={onTitleFocused}
            readOnly={locked}
            onSaved={onSaved}
          />
          <div ref={frame} className="mm-editor-frame">
            {/* La session se connecte pendant que le code de l'éditeur se charge. */}
            {editor}
            <BlockMarks
              frame={frame}
              session={session}
              {...wiring.marks}
              onComment={hoverComment}
            />
          </div>
        </article>
        {wiring.panel && (
          <CommentsPanel
            {...wiring.panel}
            comments={comments}
            level={node.level}
            trashed={trashed}
            readOnly={readOnly}
            selfId={userId}
          />
        )}
      </div>
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

/**
 * Une coupure de la synchro plus courte que cela ne se dit pas (ms). Le serveur ferme les
 * connexions de synchro d'une page quand ses droits changent (un accès donné ou retiré, la
 * corbeille…), et l'onglet revient dans le quart de seconde : ce n'est pas une panne.
 */
const CUT_NOTICE_MS = 1_500;

/** État de la connexion, quand il mérite d'être dit. */
function SyncState({ session }: { session: CollabSession }) {
  const subscribe = useCallback((onChange: () => void) => session.subscribe(onChange), [session]);
  const state = useSyncExternalStore(subscribe, () => `${session.status}:${session.synced}`);
  const [status, synced] = state.split(":");
  /** La synchro a été faite, et la connexion n'est plus là (coupée, ou en train de revenir). */
  const cut = synced === "true" && status !== "connected";
  /** La coupure dure. */
  const [lasting, setLasting] = useState(false);
  useEffect(() => {
    if (!cut) {
      setLasting(false);
      return;
    }
    const timer = setTimeout(() => setLasting(true), CUT_NOTICE_MS);
    return () => clearTimeout(timer);
  }, [cut, session]);

  let text = "";
  if (synced !== "true") {
    text = status === "disconnected" ? "Hors ligne" : "Connexion…";
  } else if (cut && lasting) {
    // En lecture seule, rien ne partira au retour du réseau.
    text = session.readOnly
      ? "Hors ligne"
      : "Hors ligne : tes modifications partiront au retour du réseau";
  }
  return (
    <span className="mm-topbar-status" role="status">
      {text}
    </span>
  );
}
