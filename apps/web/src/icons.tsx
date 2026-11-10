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
