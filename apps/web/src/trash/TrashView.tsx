// La corbeille d'un espace (`/s/:spaceId/trash`) : les pages mises à la corbeille, la plus
// récente d'abord, avec « Restaurer » et « Supprimer définitivement ». Une page de
// l'application, barre latérale à gauche, contenu centré, dans le dessin des réglages de
// l'espace.
//
// Le serveur ne liste que les pages jetées elles-mêmes (leurs sous-pages n'y sont que par
// elles, et reviennent avec elles), et seulement celles que la personne peut restaurer
// (`edit`). Supprimer définitivement demande l'accès complet (`full`), et une confirmation
// dans la page (`Confirm`) : la page part avec ses sous-pages et leur contenu, pour de bon.
// Le serveur fait foi : son refus s'affiche tel quel, la liste est relue, et la barre latérale
// avec elle (`onChanged`). La liste se relit aussi quand une page est jetée ou restaurée
// ailleurs dans l'application (`version`).
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import { deletePage, getTrash, isSessionLost, restorePage } from "../api";
import { PageIcon } from "../icons";
import { pagePath } from "../router";
import { FormError, messageOf, useDocumentTitle } from "../session/forms";
import { Confirm } from "../ui/Confirm";

type Props = {
  space: SpaceInfo;
  /** Change quand une page est jetée ou restaurée ailleurs dans l'application. */
  version: number;
  /** Une page est sortie de la corbeille, ou le serveur a refusé : l'arbre est à relire. */
  onChanged: () => void;
};

type Load =
  | { name: "loading" }
  | { name: "failed"; message: string }
  | { name: "ready"; pages: PageNode[] };

const trashedOn = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "long",
  hour: "2-digit",
  minute: "2-digit",
});

export function TrashView({ space, version, onChanged }: Props) {
  useDocumentTitle("Corbeille");
  const titleRef = useRef<HTMLHeadingElement>(null);
  const [load, setLoad] = useState<Load>({ name: "loading" });
  const [error, setError] = useState<string | null>(null);
  /** La page en cours de restauration : un double clic n'envoie qu'une requête. */
  const [restoring, setRestoring] = useState<string | null>(null);
  /** La page à supprimer, le temps de la confirmation. */
  const [deleting, setDeleting] = useState<PageNode | null>(null);
  const [busy, setBusy] = useState(false);
  /** Compte les fermetures de la confirmation, pour rattraper le focus après chacune. */
  const [closed, setClosed] = useState(0);

  const reload = useCallback(async () => {
    try {
      setLoad({ name: "ready", pages: await getTrash(space.id) });
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; la liste garde ce qu'elle a.
      if (!isSessionLost(e)) setLoad({ name: "failed", message: messageOf(e) });
    }
  }, [space.id]);

  useEffect(() => {
    void reload();
  }, [reload, version]);

  // La fenêtre fermée rend le focus au bouton qui l'a ouverte (`Confirm`). Parti avec sa
  // ligne, ce bouton n'est plus là : le focus va au titre de la page, pas dans le vide.
  useEffect(() => {
    if (closed === 0) return;
    const active = document.activeElement;
    if (!active || active === document.body) titleRef.current?.focus();
  }, [closed]);

  const restore = async (page: PageNode) => {
    if (restoring) return;
    setRestoring(page.id);
    setError(null);
    try {
      await restorePage(page.id);
      onChanged();
    } catch (e) {
      // Une page parente est à la corbeille (`409 parent_trashed`), ou la page n'y est plus.
      if (!isSessionLost(e)) {
        setError(messageOf(e));
        onChanged();
      }
    }
    // Réussie ou refusée : la corbeille telle que le serveur la voit.
    await reload();
    setRestoring(null);
  };

  const remove = async () => {
    if (!deleting) return;
    setBusy(true);
    setError(null);
    try {
      await deletePage(deleting.id);
    } catch (e) {
      // Restaurée ailleurs, peut-être (`409 not_trashed`) : elle est revenue dans l'arbre.
      if (!isSessionLost(e)) {
        setError(messageOf(e));
        onChanged();
      }
    }
    await reload();
    setBusy(false);
    setDeleting(null);
    setClosed((n) => n + 1);
  };

  let content;
  if (load.name === "loading") {
    content = <p className="mm-settings-empty">Chargement…</p>;
  } else if (load.name === "failed") {
    content = (
      <>
        <FormError message={load.message} />
        <p className="mm-settings-actions">
          <button type="button" className="mm-button" onClick={() => void reload()}>
            Réessayer
          </button>
        </p>
      </>
    );
  } else if (load.pages.length === 0) {
    content = <p className="mm-settings-empty">La corbeille est vide.</p>;
  } else {
    content = (
      <ul className="mm-people" aria-label="Pages à la corbeille">
        {load.pages.map((page) => (
          <li key={page.id} className="mm-person">
            <span className="mm-person-tile">
              <PageIcon size={16} />
            </span>
            <span className="mm-person-names">
              <a className="mm-person-name mm-trash-link" href={pagePath(page.id)}>
                {page.title}
              </a>
              {page.trashedAt !== null && (
                <span className="mm-person-detail">
                  À la corbeille depuis le {trashedOn.format(new Date(page.trashedAt))}
                </span>
              )}
            </span>
            <span className="mm-trash-actions">
              <button
                type="button"
                className="mm-button mm-button-small"
                aria-disabled={restoring !== null || undefined}
                onClick={() => void restore(page)}
              >
                Restaurer
              </button>
              {page.level === "full" && (
                <button
                  type="button"
                  className="mm-button mm-button-quiet"
                  onClick={() => setDeleting(page)}
                >
                  Supprimer définitivement
                </button>
              )}
            </span>
          </li>
        ))}
      </ul>
    );
  }

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">
          <span className="mm-topbar-crumb">{space.name} / </span>
          Corbeille
        </span>
      </header>
      <div className="mm-settings">
        <h1 ref={titleRef} className="mm-settings-title" tabIndex={-1}>
          Corbeille
        </h1>
        <p className="mm-settings-lead">
          Une page mise à la corbeille y reste, avec ses sous-pages, tant qu'on ne la restaure pas
          ou qu'on ne la supprime pas définitivement. Elle se lit encore, et ne s'écrit plus.
        </p>
        <div className="mm-trash">
          <FormError message={error} />
          {content}
        </div>
        {deleting && (
          <Confirm
            // Espaces insécables : le titre ne se coupe ni de ses guillemets ni du « ? ».
            title={`Supprimer définitivement «\u00a0${deleting.title}\u00a0»\u00a0?`}
            confirmLabel="Supprimer définitivement"
            destructive
            busy={busy}
            onConfirm={() => void remove()}
            onCancel={() => setDeleting(null)}
          >
            La page et ses sous-pages seront supprimées pour de bon, avec tout leur contenu. Cette
            action ne s'annule pas.
          </Confirm>
        )}
      </div>
    </>
  );
}
