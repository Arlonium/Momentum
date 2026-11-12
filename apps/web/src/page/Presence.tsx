// Qui a la page ouverte : les avatars des autres comptes dans la barre du haut, d'après la liste
// que le serveur envoie sur la connexion de synchro (`CollabSession.roster`, R136). Elle vient du
// serveur, pas de la présence des clients : un lecteur y figure aussi, et le nom est celui du
// compte. Les noms sont montrés comme du texte, jamais comme du HTML.
import { useCallback, useSyncExternalStore } from "react";
import type { CollabSession } from "../editor/collab";
import { colorOf } from "../session/session";

/** Avatars montrés avant le « +N ». */
const MAX_AVATARS = 5;

type Props = {
  session: Pick<CollabSession, "roster" | "subscribe">;
  /** Le compte de cet onglet : il ne s'affiche pas lui-même. */
  selfId: string;
};

export function Presence({ session, selfId }: Props) {
  const subscribe = useCallback((onChange: () => void) => session.subscribe(onChange), [session]);
  // La liste est remplacée, jamais modifiée sur place : sa référence est le cliché.
  const roster = useSyncExternalStore(subscribe, () => session.roster);
  const others = roster.filter((user) => user.id !== selfId);
  if (others.length === 0) return null;
  const shown = others.slice(0, MAX_AVATARS);
  const hidden = others.length - shown.length;
  return (
    <ul className="mm-presence" aria-label="Présents sur la page">
      {shown.map((user) => {
        const label = user.canWrite ? user.name : `${user.name} (lecture)`;
        return (
          <li
            key={user.id}
            className="mm-avatar"
            role="img"
            title={label}
            aria-label={label}
            style={{ backgroundColor: colorOf(user.id) }}
          >
            {Array.from(user.name.trim())[0]?.toLocaleUpperCase() ?? "?"}
          </li>
        );
      })}
      {hidden > 0 && (
        <li
          className="mm-avatar mm-avatar-more"
          role="img"
          title={`${hidden} ${hidden === 1 ? "autre personne" : "autres personnes"}`}
          aria-label={`${hidden} ${hidden === 1 ? "autre personne" : "autres personnes"}`}
        >
          +{hidden}
        </li>
      )}
    </ul>
  );
}
