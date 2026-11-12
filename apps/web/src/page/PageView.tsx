// Une page ouverte : barre du haut (titre, état de la connexion, présents, « Commentaires »,
// « Partager » pour qui a l'accès complet), bandeaux (lecture seule, corbeille) si besoin,
// titre, puis l'éditeur en direct ; à droite, le panneau des commentaires quand il est ouvert.
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
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { isSessionLost } from "../api";
import { openCount } from "../comments/arrange";
import { BlockMarks, blockText, cssString, shorten } from "../comments/BlockMarks";
import { CommentsPanel, type PanelTarget } from "../comments/CommentsPanel";
import { useComments } from "../comments/useComments";
import { CollabSession } from "../editor/collab";
import { requestedFixture } from "../editor/fixtures";
import { LazyEditor } from "../editor/LazyEditor";
import { ReaderBanner, ReadOnlyBanner } from "../editor/ReadOnlyBanner";
import type { SchemaCheck } from "../editor/schemaGate";
import { CommentIcon, TrashIcon } from "../icons";
import { messageOf } from "../session/forms";
import { colorOf, useSession } from "../session/session";
import { useAfterLogin } from "../session/useAfterLogin";
import { canComment, canEdit } from "../share/levels";
import { ShareMenu } from "../share/ShareMenu";
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

  // Les fils de commentaires, relus à chaque trame `comments` de la session de synchro. Un refus
  // du serveur (un droit, ou la page, ont changé entre-temps) fait aussi relire la page : son
  // niveau et la corbeille décident des boutons du panneau.
  const comments = useComments(id, session, { onRefused: () => void reread() });
  const panelId = useId();
  const commentsButton = useRef<HTMLButtonElement>(null);
  /** Autour de l'éditeur : les blocs de la page y sont cherchés par leur identifiant. */
  const frame = useRef<HTMLDivElement>(null);
  /** Le panneau des commentaires, et ce sur quoi il s'est ouvert ; `null` : fermé. */
  const [panel, setPanel] = useState<{ target: PanelTarget | null } | null>(null);
  /** Ce qui avait le focus avant que le panneau s'ouvre : il le retrouve à la fermeture. */
  const returnFocus = useRef<HTMLElement | null>(null);
  /** Numérote les demandes faites au panneau (un fil, un bloc à commenter, un bloc à montrer). */
  const requests = useRef(0);
  const panelOpen = panel !== null;
  const openPanel = (target: PanelTarget | null) => {
    if (!panelOpen) {
      const active = document.activeElement;
      returnFocus.current = active instanceof HTMLElement ? active : null;
    }
    setPanel({ target });
  };
  const closePanel = () => {
    const back = returnFocus.current;
    returnFocus.current = null;
    setPanel(null);
    const panelElement = document.getElementById(panelId);
    if (back !== null && back.isConnected && !panelElement?.contains(back)) back.focus();
    else commentsButton.current?.focus();
  };
  const openOnThread = (threadId: string) =>
    openPanel({ kind: "thread", threadId, seq: ++requests.current });
  const commentBlockNow = useRef((_: string) => {});
  commentBlockNow.current = (blockId: string) =>
    openPanel({ kind: "block", blockId, seq: ++requests.current });
  /** « Commenter » sur un bloc : le panneau s'ouvre sur son commentaire neuf. Stable. */
  const commentBlock = useCallback((blockId: string) => commentBlockNow.current(blockId), []);

  // Ce que les pastilles de la marge suivent : les blocs des fils, et les fils ouverts de chacun.
  const { threads } = comments;
  const blockIds = useMemo(
    () => [...new Set(threads?.flatMap((t) => (t.blockId === null ? [] : [t.blockId])))],
    [threads],
  );
  const marks = useMemo(() => {
    const open = new Map<string, string[]>();
    for (const t of threads ?? []) {
      if (t.blockId === null || t.resolvedAt !== null) continue;
      open.set(t.blockId, [...(open.get(t.blockId) ?? []), t.id]);
    }
    return open;
  }, [threads]);
  /** Les blocs commentés qui sont dans la page, dans l'ordre ; `null` : pas encore lus. */
  const [order, setOrder] = useState<string[] | null>(null);
  const [highlight, setHighlight] = useState<{ blockId: string; seq: number } | null>(null);
  /** L'élément d'un bloc de la page, cherché à la demande (un clic, pas une frappe). */
  const blockElement = (blockId: string) =>
    frame.current?.querySelector<HTMLElement>(
      `[data-node-type="blockOuter"][data-id=${cssString(blockId)}]`,
    ) ?? null;
  const excerpt = (blockId: string) => {
    const element = blockElement(blockId);
    const text = element === null ? "" : blockText(element);
    return text === "" ? null : shorten(text, EXCERPT_CHARS);
  };
  const showBlock = (blockId: string) => {
    blockElement(blockId)?.scrollIntoView({ block: "center", behavior: "smooth" });
    setHighlight({ blockId, seq: ++requests.current });
  };
  /**
   * L'éditeur est en lecture seule alors que la personne peut commenter : BlockNote n'y montre
   * pas la poignée des blocs, et « Commenter ce bloc » suit alors le bloc survolé.
   */
  const commentsWritable = canComment(node.level) && !trashed && !readOnly;
  const hoverComment = commentsWritable && locked ? commentBlock : null;
  /** « Commenter » dans le menu de la poignée d'un bloc, quand l'éditeur se modifie. */
  const menuComment = commentsWritable && !locked ? commentBlock : null;
  // L'éditeur ne se rend pas de nouveau quand les fils, le panneau ou les pastilles changent.
  const editor = useMemo(
    () => session && <LazyEditor session={session} fixture={fixture} onComment={menuComment} />,
    [session, fixture, menuComment],
  );
  const count = threads === null ? null : openCount(threads);

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">{node.title}</span>
        {session && <SyncState session={session} />}
        {session && <Presence session={session} selfId={userId} />}
        <button
          ref={commentsButton}
          type="button"
          className="mm-button mm-comments-button"
          // Le nombre, en toutes lettres pour les lecteurs d'écran ; le nom commence par ce
          // qui se lit sur le bouton.
          aria-label={count === null ? undefined : `Commentaires, ${openThreadsText(count)}`}
          aria-expanded={panelOpen}
          aria-controls={panelOpen ? panelId : undefined}
          onClick={() => (panelOpen ? closePanel() : openPanel(null))}
        >
          <CommentIcon />
          <span>Commentaires</span>
          {count !== null && count > 0 && <span className="mm-count">{count}</span>}
        </button>
        {/* Les accès ne se voient et ne se changent qu'avec l'accès complet, et rien ne s'y
            change à la corbeille, ni depuis un onglet qui n'écrit plus (R35). */}
        {node.level === "full" && !trashed && !readOnly && (
          <ShareMenu node={node} onChanged={onShared} onAccessLost={accessLost} />
        )}
      </header>
      <div className="mm-page-body">
        <article className="mm-page">
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
              blockIds={blockIds}
              marks={marks}
              onOrder={setOrder}
              onOpenThread={openOnThread}
              highlight={highlight}
              onComment={hoverComment}
            />
          </div>
        </article>
        {panel && (
          <CommentsPanel
            id={panelId}
            comments={comments}
            order={order}
            level={node.level}
            trashed={trashed}
            readOnly={readOnly}
            selfId={userId}
            target={panel.target}
            excerpt={excerpt}
            onShowBlock={showBlock}
            onTargetDone={() => setPanel((current) => current && { target: null })}
            onClose={closePanel}
          />
        )}
      </div>
    </>
  );
}

/** Le début du texte d'un bloc, au-dessus de son commentaire neuf : au plus tant de caractères. */
const EXCERPT_CHARS = 120;

/** Le nombre de fils ouverts, en toutes lettres pour les lecteurs d'écran. */
function openThreadsText(count: number): string {
  if (count === 0) return "aucun fil ouvert";
  return count === 1 ? "1 fil ouvert" : `${count} fils ouverts`;
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
