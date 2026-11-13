// Le menu « … » de la barre du haut d'une page : les actions sur la page qui n'ont pas de bouton
// à elles. Pour l'instant « Historique » (R135), à partir de « Peut modifier » : le serveur
// l'exige, pour qu'un lecteur ne lise pas ce qui a été effacé avant qu'on partage la page avec
// lui (S5). À la corbeille aussi, et depuis un onglet d'un autre schéma que le serveur (R35) ;
// c'est la fenêtre qui décide si l'on peut restaurer. Sans entrée pour le niveau de la personne,
// pas de bouton. D'autres entrées s'y ajouteront.
//
// Le niveau peut tomber pendant que la fenêtre est ouverte : elle reste ouverte, avec le refus du
// serveur tel quel, jusqu'à ce qu'on la ferme ; le focus revient alors à la page
// (`focusFallback`) si le bouton du menu n'est plus là.
//
// Au clavier, c'est le menu des lignes de l'arbre (`RowMenu`) : il prend le focus sur sa première
// entrée, les flèches passent de l'une à l'autre, Échap et Tab le ferment. La fenêtre
// « Historique » est chargée à la demande : son code, et celui de l'aperçu, ne pèsent pas sur
// l'ouverture d'une page. Il est demandé dès que le menu s'ouvre, pour être là au clic. Fermée,
// la fenêtre rend le focus au bouton du menu. Une version restaurée est annoncée par une région
// `role="status"` (invisible : le contenu qui change suffit à l'œil), posée avant l'annonce.
import type { PageNode } from "@mementum/protocol";
import {
  Component,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { HistorySession } from "../history/HistoryDialog";
import { HistoryIcon, MoreIcon } from "../icons";
import { canEdit } from "../share/levels";
import { RowMenu, type MenuAction } from "../sidebar/RowMenu";

/** Charge le module de la fenêtre « Historique » (le navigateur ne le charge qu'une fois). */
function loadHistory() {
  return import("../history/HistoryDialog");
}

const HistoryDialog = lazy(async () => ({ default: (await loadHistory()).HistoryDialog }));

const LABEL = "Actions de la page";

type Props = {
  node: PageNode;
  /** La session de synchro de la page ; `null` entre deux sessions. */
  session: HistorySession | null;
  /** L'onglet n'a plus le schéma du serveur (R35) : rien ne s'écrit d'ici. */
  readOnly: boolean;
  /** Le compte de la session. */
  selfId: string;
  /** Tenue quand le compte revient, après une session perdue (`useAfterLogin`). */
  afterLogin?: () => Promise<void>;
  /**
   * La fenêtre se ferme et le bouton du menu n'est plus là (plus aucune entrée pour ce niveau) :
   * où rendre le focus, pour qu'il ne tombe pas sur `body`.
   */
  focusFallback?: () => void;
  /** Le serveur a refusé une action du menu : la page est à relire. */
  onRefused: () => void;
};

export function PageMenu({
  node,
  session,
  readOnly,
  selfId,
  afterLogin,
  focusFallback,
  onRefused,
}: Props) {
  /** Le bouton du menu ouvert ; `null` : fermé. */
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [history, setHistory] = useState(false);
  /** Ce qu'annonce la région d'état (un lecteur d'écran la lit quand elle change). */
  const [announced, setAnnounced] = useState("");
  const buttonRef = useRef<HTMLButtonElement>(null);
  /** La fenêtre vient de se fermer : le focus revient au bouton, une fois qu'elle n'est plus là. */
  const closed = useRef(false);

  const closeMenu = useCallback(() => setAnchor(null), []);
  const announceRestored = useCallback(() => setAnnounced("Version restaurée."), []);
  const closeHistory = useCallback(() => {
    closed.current = true;
    setHistory(false);
  }, []);

  // Après le retrait de la fenêtre : tant qu'elle est ouverte, le reste de la page est inerte, et
  // le bouton ne prendrait pas le focus.
  useEffect(() => {
    if (history || !closed.current) return;
    closed.current = false;
    if (buttonRef.current?.isConnected) buttonRef.current.focus();
    else focusFallback?.();
  }, [history, focusFallback]);

  const actions: MenuAction[] = [];
  const historyOffered = canEdit(node.level);
  // Le niveau est tombé, menu ouvert : il n'a plus rien à proposer, il se ferme.
  if (!historyOffered && anchor !== null) setAnchor(null);
  if (historyOffered) {
    actions.push({
      label: "Historique",
      icon: <HistoryIcon />,
      onSelect: () => {
        setAnnounced("");
        setHistory(true);
      },
    });
  }

  return (
    <>
      <p className="mm-visually-hidden" role="status">
        {announced}
      </p>
      {actions.length > 0 && (
        <button
          ref={buttonRef}
          type="button"
          className="mm-icon-button"
          aria-label={LABEL}
          aria-haspopup="menu"
          aria-expanded={anchor !== null}
          title={LABEL}
          onClick={(event) => {
            // Un clic sur le bouton d'un menu ouvert le ferme (`RowMenu` le laisse passer).
            if (anchor !== null) {
              setAnchor(null);
              return;
            }
            // Le code de la fenêtre est demandé tout de suite, pour être là au clic. Un échec se
            // redira au clic, dans la fenêtre d'erreur.
            void loadHistory().catch(() => {});
            setAnchor(event.currentTarget);
          }}
        >
          <MoreIcon />
        </button>
      )}
      {anchor && actions.length > 0 && (
        <RowMenu label={LABEL} anchor={anchor} actions={actions} onClose={closeMenu} />
      )}
      {history && (
        <HistoryBoundary onClose={closeHistory}>
          <Suspense fallback={null}>
            <HistoryDialog
              node={node}
              session={session}
              readOnly={readOnly}
              selfId={selfId}
              afterLogin={afterLogin}
              onRefused={onRefused}
              onRestored={announceRestored}
              onClose={closeHistory}
            />
          </Suspense>
        </HistoryBoundary>
      )}
    </>
  );
}

/**
 * Code de la fenêtre introuvable (réseau coupé, application mise à jour depuis) ou fenêtre en
 * erreur : un message, au lieu de rien, et la page reste en place.
 */
class HistoryBoundary extends Component<
  { onClose: () => void; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown): void {
    console.error("Mementum : la fenêtre « Historique » n'a pas pu s'afficher.", error);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return <HistoryFailed onClose={this.props.onClose} />;
  }
}

function HistoryFailed({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      role="alertdialog"
      className="mm-dialog"
      aria-label="Historique"
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClose={onClose}
    >
      <h2 className="mm-dialog-title">Historique</h2>
      <p className="mm-dialog-lead">
        L'historique n'a pas pu s'afficher. Recharge la page pour réessayer.
      </p>
      <div className="mm-dialog-actions">
        <button type="button" className="mm-button" onClick={onClose}>
          Fermer
        </button>
        <button type="button" className="mm-button mm-button-primary" onClick={() => location.reload()}>
          Recharger
        </button>
      </div>
    </dialog>
  );
}
