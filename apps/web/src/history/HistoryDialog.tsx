// La fenêtre « Historique » d'une page (R135), ouverte par le menu « … » de la barre du haut
// (`PageMenu`), chargée à la demande. À gauche, la liste : « Version actuelle », puis chaque
// version, la plus récente d'abord, dans l'ordre du serveur (sa date, « il y a… », les comptes
// qui l'ont modifiée, « avant une restauration » pour ce qu'une restauration a remplacé). À
// droite, l'aperçu de celle qu'on choisit, en lecture seule, dans le même éditeur que la page
// (`VersionPreview`, qui n'écrit jamais). En bas, « Restaurer cette version ».
//
// Ce qui se lit, et quand : la liste à l'ouverture, puis à chaque trame `versions` (`useVersions`) ;
// l'état d'une version seulement quand on la choisit. À l'ouverture, la version actuelle est
// choisie : rien n'est demandé de plus. Une version choisie qui sort de la liste (une page garde
// ses cent dernières) ferme son aperçu, et une phrase le dit.
//
// Restaurer demande « Peut modifier », hors corbeille, depuis un onglet qui a le schéma du
// serveur (R35) : sinon le bouton n'existe pas, et une phrase dit pourquoi. Une confirmation le
// précède : le contenu actuel sera remplacé, et restera dans l'historique ; quand d'autres ont la
// page ouverte (la liste des présents de la session, hors soi), elle dit aussi que ce qu'ils
// tapent pendant la restauration peut être perdu. Pendant la requête (plusieurs secondes sur une
// grande page), son bouton dit « Restauration… », et un second clic n'en demande pas une autre.
// Restaurée, la version arrive à tous les onglets par la synchro, la page l'annonce (`PageMenu`)
// et la fenêtre se ferme. Un refus du serveur s'affiche tel quel, et la page est relue.
//
// Les noms des auteurs viennent d'autres comptes : du texte, chacun isolé (`dir="auto"`,
// `unicode-bidi: isolate`), et entouré de U+2068 et U+2069 dans un attribut.
//
// Au clavier : le focus entre dans la fenêtre, sur la version choisie ; Tab et Maj+Tab tournent
// sans en sortir ; les flèches, Début et Fin parcourent la liste, Entrée choisit ; Échap ferme.
// C'est le menu qui rend le focus à son bouton.
import type { PageNode, Version } from "@mementum/protocol";
import {
  Fragment,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import type * as Y from "yjs";
import { ApiError, isSessionLost, restoreVersion } from "../api";
import { isolate } from "../comments/dom";
import { fullDate, isoDate, relativeTime, useNow } from "../comments/time";
import type { CollabSession, RosterUser } from "../editor/collab";
import { CloseIcon } from "../icons";
import { initialOf, inkOn } from "../page/avatarInk";
import { FormError, messageOf } from "../session/forms";
import { colorOf } from "../session/session";
import { canEdit } from "../share/levels";
import { Confirm } from "../ui/Confirm";
import { useVersions, type VersionsSession } from "./useVersions";
import { VersionPreview } from "./VersionPreview";

/** Ce que la fenêtre lit de la session de synchro de la page. */
export type HistorySession = VersionsSession &
  Pick<CollabSession, "roster"> & {
    /** Le document de la page tel que le serveur le connaît : la version actuelle. */
    readonly provider: { readonly doc: Y.Doc };
  };

type Props = {
  node: PageNode;
  /** La session de synchro de la page ; `null` entre deux sessions. */
  session: HistorySession | null;
  /** L'onglet n'a plus le schéma du serveur (R35) : rien ne s'écrit d'ici. */
  readOnly: boolean;
  /** Le compte de la session. */
  selfId: string;
  /**
   * Tenue quand le compte revient, après une session perdue (`useAfterLogin`) : une liste
   * refusée par `401` est alors relue.
   */
  afterLogin?: () => Promise<void>;
  /** Le serveur a refusé une restauration (`403`, `404`, `409`) : la page est à relire. */
  onRefused: () => void;
  /** Une version vient d'être restaurée : la page l'annonce (la fenêtre se ferme ensuite). */
  onRestored: () => void;
  /** La fenêtre se ferme : Échap, « Fermer », un clic sur le voile, une restauration faite. */
  onClose: () => void;
};

export const RESTORE_REPLACES = "Le contenu actuel sera remplacé. Il restera dans l'historique.";
export const RESTORE_OTHERS =
  "D'autres personnes ont cette page ouverte : ce qu'elles tapent pendant la restauration peut être perdu.";

/** Avatars d'auteurs montrés avant le « +N ». */
const MAX_AVATARS = 3;

const NOBODY: RosterUser[] = [];

/** Des comptes autres que `selfId` ont-ils la page ouverte ? Suit la liste des présents. */
function useOthersPresent(session: HistorySession | null, selfId: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => (session ? session.subscribe(onChange) : () => {}),
    [session],
  );
  // La liste est remplacée, jamais modifiée sur place : sa référence est le cliché.
  const roster = useSyncExternalStore(subscribe, () => session?.roster ?? NOBODY);
  return roster.some((user) => user.id !== selfId);
}

/**
 * La session a-t-elle fait sa première synchro ? Avant, son document est vide (une session
 * neuve : fenêtre ouverte tôt, ou page passée en lecture seule ou revenue de la corbeille), et sa
 * copie montrerait une page vide. Appris par la session (`subscribe`), jamais par un écouteur
 * posé sur son document.
 */
function useSynced(session: HistorySession | null): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => (session ? session.subscribe(onChange) : () => {}),
    [session],
  );
  return useSyncExternalStore(subscribe, () => session?.synced ?? false);
}

export function HistoryDialog({
  node,
  session,
  readOnly,
  selfId,
  afterLogin,
  onRefused,
  onRestored,
  onClose,
}: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const titleId = useId();
  const previewTitleId = useId();
  const { versions, state, error, reload } = useVersions(node.id, session, { afterLogin });
  /** La version choisie ; `null` : la version actuelle. */
  const [chosen, setChosen] = useState<string | null>(null);
  /**
   * Compte les choix de la version actuelle : chacun reprend sa copie, figée au moment du choix
   * (R192). Rechoisir la version actuelle la remet donc à jour.
   */
  const [snapshot, setSnapshot] = useState(0);
  const [confirming, setConfirming] = useState(false);
  const [restoring, setRestoring] = useState(false);
  /**
   * Une restauration est partie : posé avant tout rendu, pour qu'un second clic (ou une seconde
   * Entrée) coup sur coup n'en demande pas une autre.
   */
  const sending = useRef(false);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const now = useNow();
  const othersPresent = useOthersPresent(session, selfId);
  /**
   * Le document de la page pour la version actuelle : celui de la session une fois synchronisée ;
   * `null` d'ici là (l'aperçu dit « Chargement… »). Une session remplacée attend de même.
   */
  const synced = useSynced(session);
  const current = session !== null && synced ? session.provider.doc : null;

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    dialog.showModal();
    // Le focus entre dans la fenêtre, sur la version choisie.
    listRef.current?.querySelector<HTMLButtonElement>('button[aria-current="true"]')?.focus();
  }, []);

  const version = chosen === null ? null : (versions?.find((v) => v.id === chosen) ?? null);
  /** La version choisie n'est plus dans la liste relue : elle est sortie de l'historique. */
  const gone = chosen !== null && versions !== null && version === null;
  const trashed = node.trashedAt !== null;
  const canRestore = canEdit(node.level) && !trashed && !readOnly;

  // La confirmation n'a plus d'objet : la version est sortie de l'historique, ou le droit de
  // restaurer est parti (niveau baissé, page jetée ailleurs). Pas pendant l'envoi : la réponse
  // du serveur le dira.
  if (confirming && !restoring && (version === null || !canRestore)) setConfirming(false);
  // Le bouton qui l'avait ouverte est parti avec elle : le focus ne tombe pas hors de la fenêtre.
  useEffect(() => {
    if (confirming) return;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const list = listRef.current;
    const target =
      list?.querySelector<HTMLButtonElement>('button[aria-current="true"]') ??
      list?.querySelector<HTMLButtonElement>("button");
    target?.focus();
  }, [confirming]);

  const choose = (id: string | null) => {
    setChosen(id);
    if (id === null) setSnapshot((n) => n + 1);
    setRestoreError(null);
  };

  const restore = async () => {
    if (chosen === null || sending.current) return;
    sending.current = true;
    setRestoring(true);
    try {
      await restoreVersion(node.id, chosen);
      sending.current = false;
      setRestoring(false);
      setConfirming(false);
      onRestored();
      onClose();
    } catch (e) {
      sending.current = false;
      setRestoring(false);
      setConfirming(false);
      // Session perdue : la connexion est demandée par-dessus la page ; la fenêtre attend.
      if (!isSessionLost(e)) setRestoreError(messageOf(e));
      // Un droit, la page ou la version ont pu changer depuis que l'écran les a lus.
      if (e instanceof ApiError && [403, 404, 409].includes(e.status)) onRefused();
      reload();
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key !== "Tab") return;
    const focusable = [
      ...event.currentTarget.querySelectorAll<HTMLElement>(
        'button:not(:disabled), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      ),
    ];
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !event.currentTarget.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !event.currentTarget.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  };

  // Les flèches parcourent la liste ; Entrée ou Espace choisissent (ce sont des boutons).
  const onListKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>(":scope > li > button")];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0) return;
    let next: HTMLButtonElement | undefined;
    switch (event.key) {
      case "ArrowDown":
        next = items[(at + 1) % items.length];
        break;
      case "ArrowUp":
        next = items[(at <= 0 ? items.length : at) - 1];
        break;
      case "Home":
        next = items[0];
        break;
      case "End":
        next = items[items.length - 1];
        break;
      default:
        return;
    }
    event.preventDefault();
    next?.focus();
  };

  // Un clic sur le voile (hors du cadre de la fenêtre) la ferme.
  const onClick = (event: MouseEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    const box = event.currentTarget.getBoundingClientRect();
    const inside =
      event.clientX >= box.left &&
      event.clientX <= box.right &&
      event.clientY >= box.top &&
      event.clientY <= box.bottom;
    if (!inside) onClose();
  };

  let hint: string | null = null;
  if (chosen === null) hint = "Choisis une version pour la voir.";
  else if (version !== null && !canRestore) {
    hint = readOnly
      ? // Ce que dit le bandeau de l'onglet (R35), pour la restauration.
        "Une nouvelle version de Mementum est disponible : recharge la page pour restaurer une version."
      : trashed
        ? "La page est à la corbeille : aucune version ne s'y restaure."
        : "Restaurer une version demande de pouvoir modifier la page.";
  }

  return (
    <>
      <dialog
        ref={ref}
        className="mm-dialog mm-history"
        aria-labelledby={titleId}
        onKeyDown={onKeyDown}
        onClick={onClick}
        // Échap : le parent retire la fenêtre. Seulement le sien : celui d'une autre fenêtre ne
        // la concerne pas.
        onCancel={(event) => {
          if (event.target !== event.currentTarget) return;
          event.preventDefault();
          onClose();
        }}
        // Chrome la ferme quand même au second Échap sans geste entre les deux.
        onClose={(event) => {
          if (event.target === event.currentTarget) onClose();
        }}
      >
        <div className="mm-history-head">
          <h2 id={titleId} className="mm-dialog-title">
            Historique
          </h2>
          <button
            type="button"
            className="mm-icon-button"
            aria-label="Fermer l'historique"
            title="Fermer (Échap)"
            onClick={onClose}
          >
            <CloseIcon />
          </button>
        </div>
        <div className="mm-history-body">
          <div className="mm-history-side">
            <ul
              ref={listRef}
              className="mm-history-list"
              aria-label="Versions de la page"
              aria-busy={versions === null && state === "loading"}
              onKeyDown={onListKeyDown}
            >
              <li>
                <button
                  type="button"
                  className="mm-history-item"
                  aria-current={chosen === null ? "true" : undefined}
                  onClick={() => choose(null)}
                >
                  <span className="mm-history-date">Version actuelle</span>
                  <span className="mm-history-detail">Le contenu de la page maintenant</span>
                </button>
              </li>
              {versions?.map((v) => (
                <li key={v.id}>
                  <VersionItem version={v} now={now} chosen={v.id === chosen} onChoose={() => choose(v.id)} />
                </li>
              ))}
            </ul>
            {versions === null && state === "loading" && (
              <p className="mm-history-note">Chargement…</p>
            )}
            {versions !== null && versions.length === 0 && (
              <p className="mm-history-note">
                Aucune version pour l'instant : elles sont gardées au fil des modifications, et
                quand plus personne n'a la page ouverte.
              </p>
            )}
            {state === "failed" && (
              <div className="mm-history-read-error">
                <FormError
                  message={`L'historique n'a pas pu être ${versions === null ? "lu" : "relu"}. ${error ?? ""}`}
                />
                <button type="button" className="mm-button mm-button-small" onClick={reload}>
                  Réessayer
                </button>
              </div>
            )}
          </div>
          <section className="mm-history-preview" aria-labelledby={previewTitleId}>
            <h3 id={previewTitleId} className="mm-history-preview-title">
              {chosen === null
                ? "Version actuelle"
                : version !== null
                  ? fullDate(version.createdAt)
                  : "Version introuvable"}
            </h3>
            <div className="mm-history-preview-body">
              {gone ? (
                <p className="mm-history-note" role="status">
                  Cette version n'est plus dans l'historique : une page garde ses cent dernières
                  versions.
                </p>
              ) : (
                <VersionPreview
                  pageId={node.id}
                  versionId={chosen}
                  current={current}
                  snapshot={snapshot}
                  onMissing={reload}
                />
              )}
            </div>
          </section>
        </div>
        <div className="mm-history-foot">
          <div className="mm-history-foot-text">
            <FormError message={restoreError} />
            {restoreError === null && hint && <p className="mm-history-hint">{hint}</p>}
          </div>
          {canRestore && version !== null && (
            <button
              type="button"
              className="mm-button mm-button-primary"
              aria-disabled={restoring || undefined}
              onClick={() => {
                if (restoring) return;
                setRestoreError(null);
                setConfirming(true);
              }}
            >
              Restaurer cette version
            </button>
          )}
        </div>
      </dialog>
      {/* Hors de la fenêtre dans l'arbre de React : son Échap et ses événements ne remontent
          pas jusqu'à elle. */}
      {confirming && (
        <Confirm
          // Espace insécable : le « ? » ne passe pas seul à la ligne.
          title={"Restaurer cette version ?"}
          // Plusieurs secondes sur une grande page (toute la page est réécrite) : le bouton le dit.
          confirmLabel={restoring ? "Restauration…" : "Restaurer"}
          busy={restoring}
          onConfirm={() => void restore()}
          onCancel={() => setConfirming(false)}
        >
          {othersPresent ? (
            <>
              <p>{RESTORE_REPLACES}</p>
              <p>{RESTORE_OTHERS}</p>
            </>
          ) : (
            RESTORE_REPLACES
          )}
        </Confirm>
      )}
    </>
  );
}

type ItemProps = {
  version: Version;
  /** L'instant présent (millisecondes Unix), pour « il y a… ». */
  now: number;
  chosen: boolean;
  onChoose: () => void;
};

/** Une version dans la liste : sa date, son âge, sa raison s'il y a lieu, ses auteurs. */
function VersionItem({ version, now, chosen, onChoose }: ItemProps) {
  const { authors } = version;
  const shown = authors.slice(0, MAX_AVATARS);
  const hidden = authors.length - shown.length;
  return (
    <button
      type="button"
      className="mm-history-item"
      aria-current={chosen ? "true" : undefined}
      onClick={onChoose}
    >
      <span className="mm-history-date">
        <time dateTime={isoDate(version.createdAt)}>{fullDate(version.createdAt)}</time>
      </span>
      <span className="mm-history-detail">
        {relativeTime(version.createdAt, now)}
        {version.reason === "before_restore" && (
          <span className="mm-tag mm-history-tag">avant une restauration</span>
        )}
      </span>
      {authors.length > 0 && (
        <span
          className="mm-history-authors"
          title={authors.map((author) => isolate(author.name)).join(", ")}
        >
          <span className="mm-history-avatars" aria-hidden="true">
            {shown.map((author) => {
              const color = colorOf(author.id);
              return (
                <span
                  key={author.id}
                  className="mm-history-avatar"
                  style={{ backgroundColor: color, color: inkOn(color) }}
                >
                  {initialOf(author.name)}
                </span>
              );
            })}
            {hidden > 0 && <span className="mm-history-more">+{hidden}</span>}
          </span>
          <span className="mm-history-names">
            {authors.map((author, i) => (
              <Fragment key={author.id}>
                {i > 0 && ", "}
                <span className="mm-history-author mm-isolate" dir="auto">
                  {author.name}
                </span>
              </Fragment>
            ))}
          </span>
        </span>
      )}
    </button>
  );
}
