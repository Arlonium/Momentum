// Icônes vectorielles de l'interface (design A « Papier » : jamais d'emoji), au trait de
// la maquette `docs/design/maquettes/Main.dc.html`.
import type { ReactNode } from "react";

function Icon({ size = 16, children }: { size?: number; children: ReactNode }) {
  return (
    <svg
      aria-hidden="true"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

export const PageIcon = ({ size = 15 }: { size?: number }) => (
  <Icon size={size}>
    <path d="M6 3h8l4 4v14H6z" />
    <path d="M14 3v4h4" />
  </Icon>
);

export const PlusIcon = ({ size = 14 }: { size?: number }) => (
  <Icon size={size}>
    <path d="M12 5v14" />
    <path d="M5 12h14" />
  </Icon>
);

export const ChevronIcon = () => (
  <Icon size={12}>
    <path d="M9 6l6 6-6 6" />
  </Icon>
);

export const InfoIcon = () => (
  <Icon size={18}>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5" />
    <path d="M12 8h.01" />
  </Icon>
);

export const WarningIcon = () => (
  <Icon size={16}>
    <path d="M12 4l9 16H3z" />
    <path d="M12 10v4" />
    <path d="M12 17h.01" />
  </Icon>
);

export const BookmarkIcon = () => (
  <Icon>
    <path d="M6 3h12v18l-6-4-6 4z" />
  </Icon>
);

export const DatabaseIcon = () => (
  <Icon>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 10h18" />
    <path d="M9 10v10" />
  </Icon>
);

export const SyncIcon = () => (
  <Icon>
    <path d="M4 12a8 8 0 0 1 14-5.3L20 9" />
    <path d="M20 4v5h-5" />
    <path d="M20 12a8 8 0 0 1-14 5.3L4 15" />
    <path d="M4 20v-5h5" />
  </Icon>
);

export const TocIcon = () => (
  <Icon>
    <path d="M9 6h12M9 12h12M9 18h12" />
    <path d="M4 6h.01M4 12h.01M4 18h.01" />
  </Icon>
);

export const KeyIcon = () => (
  <Icon>
    <circle cx="8" cy="15" r="4" />
    <path d="M11 12l9-9" />
    <path d="M16 7l3 3" />
    <path d="M14 9l2 2" />
  </Icon>
);

export const LogOutIcon = () => (
  <Icon>
    <path d="M9 4H5v16h4" />
    <path d="M16 8l4 4-4 4" />
    <path d="M20 12H9" />
  </Icon>
);

/** Réglages : une roue dentée (huit dents pleines autour d'un moyeu). */
export const SettingsIcon = () => (
  <Icon>
    <path d="M10.34 4.79L10.83 2.47h2.34l.49 2.32A7.4 7.4 0 0 1 15.92 5.72l1.99-1.28 1.65 1.65-1.28 1.99A7.4 7.4 0 0 1 19.21 10.34l2.32.49v2.34l-2.32.49a7.4 7.4 0 0 1-.93 2.26l1.28 1.99-1.65 1.65-1.99-1.28a7.4 7.4 0 0 1-2.26.93l-.49 2.32h-2.34l-.49-2.32a7.4 7.4 0 0 1-2.26-.93l-1.99 1.28-1.65-1.65 1.28-1.99a7.4 7.4 0 0 1-.93-2.26l-2.32-.49v-2.34l2.32-.49a7.4 7.4 0 0 1 .93-2.26L4.44 6.09l1.65-1.65 1.99 1.28a7.4 7.4 0 0 1 2.26-.93z" />
    <circle cx="12" cy="12" r="3" />
  </Icon>
);

/** Copier un lien (le maillon du menu Partager de la maquette). */
export const LinkIcon = () => (
  <Icon size={15}>
    <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
    <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
  </Icon>
);

export const CheckIcon = () => (
  <Icon size={15}>
    <path d="M5 12.5l4.5 4.5L19 7.5" />
  </Icon>
);

/** Une invitation envoyée à une adresse. */
export const MailIcon = () => (
  <Icon>
    <rect x="3" y="5" width="18" height="14" rx="2" />
    <path d="M3.5 6.5l8.5 6.5 8.5-6.5" />
  </Icon>
);

/** Ouvre un menu vers le bas (sélecteur d'espace de la maquette). */
export const ChevronDownIcon = () => (
  <Icon size={14}>
    <path d="M6 9l6 6 6-6" />
  </Icon>
);

/** « Nouvelle page » : le crayon de la maquette, à côté du sélecteur d'espace. */
export const ComposeIcon = () => (
  <Icon size={17}>
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
  </Icon>
);

/** La corbeille de la maquette. */
export const TrashIcon = () => (
  <Icon>
    <path d="M4 7h16" />
    <path d="M9 7V4h6v3" />
    <path d="M6 7l1 13h10l1-13" />
  </Icon>
);

/** « … » : les actions d'une ligne (les trois points pleins de la maquette). */
export const MoreIcon = () => (
  <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
    <circle cx="5" cy="12" r="1.6" />
    <circle cx="12" cy="12" r="1.6" />
    <circle cx="19" cy="12" r="1.6" />
  </svg>
);

/** « Déplacer vers le haut », « vers le bas » : une flèche. */
export const ArrowUpIcon = () => (
  <Icon>
    <path d="M12 19V5" />
    <path d="M6 11l6-6 6 6" />
  </Icon>
);

export const ArrowDownIcon = () => (
  <Icon>
    <path d="M12 5v14" />
    <path d="M6 13l6 6 6-6" />
  </Icon>
);

/** « Imbriquer dans la page précédente » : un cran vers la droite, sous la ligne du dessus. */
export const NestIcon = () => (
  <Icon>
    <path d="M5 5v7a3 3 0 0 0 3 3h11" />
    <path d="M15 11l4 4-4 4" />
  </Icon>
);

/** « Sortir de la page parente » : un cran vers la gauche. */
export const UnnestIcon = () => (
  <Icon>
    <path d="M19 5v7a3 3 0 0 1-3 3H5" />
    <path d="M9 11l-4 4 4 4" />
  </Icon>
);

/** Les commentaires : une bulle (bouton de la barre du haut, pastilles de la marge). */
export const CommentIcon = ({ size = 16 }: { size?: number }) => (
  <Icon size={size}>
    <path d="M5 5h14a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-8l-4 3v-3H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z" />
  </Icon>
);

/** Fermer un panneau : une croix. */
export const CloseIcon = () => (
  <Icon>
    <path d="M6 6l12 12" />
    <path d="M18 6L6 18" />
  </Icon>
);

/** Ajouter une réaction : un visage, et le « + » d'un ajout. */
export const ReactIcon = () => (
  <Icon>
    <path d="M20 12a8 8 0 1 1-6-7.75" />
    <path d="M9 14a4 4 0 0 0 6 0" />
    <path d="M9.5 10h.01" />
    <path d="M14.5 10h.01" />
    <path d="M19 3v5" />
    <path d="M16.5 5.5h5" />
  </Icon>
);

/** L'historique d'une page : une horloge, et la flèche d'un retour en arrière. */
export const HistoryIcon = () => (
  <Icon>
    <path d="M3.5 12a8.5 8.5 0 1 0 2.5-6" />
    <path d="M3 4v4h4" />
    <path d="M12 8v4l3 2" />
  </Icon>
);

// Favoris (R254) : l'étoile de « Ajouter aux favoris », barrée pour « Retirer des favoris ».
export const StarIcon = () => (
  <Icon>
    <path d="M12 3.5l2.6 5.3 5.8.8-4.2 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z" />
  </Icon>
);

export const StarOffIcon = () => (
  <Icon>
    <path d="M12 3.5l2.6 5.3 5.8.8-4.2 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.2-4.1 5.8-.8z" />
    <path d="M4 4l16 16" />
  </Icon>
);

// Blocs riches (Task C1b).

/** Le callout : un encadré et sa marque, dans le menu « / » et le choix du type de bloc. */
export const CalloutIcon = ({ size = 16 }: { size?: number }) => (
  <Icon size={size}>
    <rect x="3" y="5" width="18" height="14" rx="2" />
    <path d="M7 10v4" />
    <path d="M11 10h6" />
    <path d="M11 14h4" />
  </Icon>
);

/** L'équation en bloc : la racine carrée, dans le menu « / ». */
export const EquationIcon = ({ size = 18 }: { size?: number }) => (
  <Icon size={size}>
    <path d="M3 13l3-1 3 7 5-15h7" />
  </Icon>
);

/** L'équation en ligne : la racine dans une ligne de texte. */
export const InlineEquationIcon = ({ size = 18 }: { size?: number }) => (
  <Icon size={size}>
    <path d="M3 20h18" />
    <path d="M5 11l2-1 2 5 4-11h6" />
  </Icon>
);

// Dupliquer une page (Task C10, R252).

/** « Dupliquer » : deux pages, l'une sur l'autre, dans le menu d'une ligne et celui de la page. */
export const DuplicatePageIcon = () => (
  <Icon>
    <rect x="8" y="8" width="12" height="13" rx="1.5" />
    <path d="M16 8V4.5A1.5 1.5 0 0 0 14.5 3h-9A1.5 1.5 0 0 0 4 4.5v11A1.5 1.5 0 0 0 5.5 17H8" />
  </Icon>
);

// Fichiers (Task C8).

/** « Télécharger » : une flèche vers un plateau, dans la barre de mise en forme d'un bloc de fichier. */
export const DownloadIcon = ({ size = 16 }: { size?: number }) => (
  <Icon size={size}>
    <path d="M12 4v11" />
    <path d="M7.5 11 12 15.5 16.5 11" />
    <path d="M5 19.5h14" />
  </Icon>
);
