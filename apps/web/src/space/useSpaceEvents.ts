// Le canal de l'espace (Ruling R133) : une WebSocket par espace affiché, `GET /events/{id}`,
// sur laquelle le serveur pousse un seul numéro, la version de l'espace (`{"lsn":N}`), à
// l'ouverture puis à chaque changement de ce que montre la barre latérale. Le client n'y
// envoie jamais rien : le serveur ferme une connexion qui lui écrit une trame de données.
//
// Le numéro ne dit pas ce qui a changé : quand il dépasse celui de l'arbre affiché (la version
// lue avant ses pages, donc une borne basse, R141), l'arbre est relu par l'API, déjà filtrée
// par les droits. Égal ou plus petit, rien ne part. Plusieurs trames rapprochées ne font
// qu'une relecture, `EVENTS_DEBOUNCE_MS` après la dernière, et au plus tard
// `EVENTS_MAX_WAIT_MS` après la première. La première trame d'une connexion fait relire dès
// qu'elle diffère de la version de l'arbre, plus basse comprise (R156) : une version qui
// redescend dit que la base a été restaurée sous un serveur relancé.
//
// Le canal ne passe pas par `gatedWebSocket` : il n'écrit rien dans un document. Il ne survit
// pas pour autant à ce que l'application démonte (le compte a changé, R115) : sa WebSocket est
// fermée, et ses minuteurs annulés, au démontage et quand l'espace change.
//
// Un canal tombé (refus `401` ou `404` à la poignée de main, coupure, fermeture du serveur
// quand la session finit ou qu'un rôle est retiré) revient après une seconde, puis le double à
// chaque échec, trente secondes au plus ; une trame reçue remet le délai à une seconde. Tant
// qu'il est tombé, `live` est faux, et la barre latérale se rabat sur son minuteur de dix
// secondes (R128) : ce sont les relectures existantes qui disent à l'écran ce qui se passe.
import { useEffect, useRef, useState } from "react";

/** Plusieurs trames qui se suivent ne font qu'une relecture, cette durée après la dernière. */
export const EVENTS_DEBOUNCE_MS = 250;
/**
 * Une relecture part au plus tard cette durée après la première trame qu'elle doit traiter :
 * une suite de trames plus rapprochées que l'anti-rebond ne fige pas la barre latérale.
 */
export const EVENTS_MAX_WAIT_MS = 1_000;
/** Délai avant la première reconnexion ; il double à chaque échec. */
export const RECONNECT_FIRST_MS = 1_000;
/** Le délai de reconnexion ne dépasse jamais cela. */
export const RECONNECT_MAX_MS = 30_000;

/** `ws(s)://<hôte de la page>/events/<espace>` : le même hôte que l'API, par le mandataire. */
function eventsUrl(spaceId: string): string {
  return `${location.origin.replace(/^http/, "ws")}/events/${encodeURIComponent(spaceId)}`;
}

/** Le numéro d'une trame du serveur ; `null` pour tout ce qui n'a pas la forme attendue. */
function lsnOf(data: unknown): number | null {
  if (typeof data !== "string") return null;
  try {
    const lsn = (JSON.parse(data) as { lsn?: unknown } | null)?.lsn;
    return typeof lsn === "number" && Number.isSafeInteger(lsn) && lsn >= 0 ? lsn : null;
  } catch {
    return null;
  }
}

/**
 * Ouvre le canal de l'espace `spaceId` (`null` : aucun). `onChange(lsn)` est appelé, avec la
 * plus haute version reçue, quand elle dépasse `known()`, la version de l'arbre affiché
 * (`null` : pas d'arbre encore ; c'est à l'appelant de ne pas jeter la lecture en route).
 * `live` : le canal est ouvert.
 */
export function useSpaceEvents(
  spaceId: string | null,
  onChange: (lsn: number) => void,
  known: () => number | null = () => null,
): { live: boolean } {
  const [live, setLive] = useState(false);
  // Les rappels du dernier rendu : en changer ne referme pas le canal.
  const callbacks = useRef({ onChange, known });
  callbacks.current = { onChange, known };

  useEffect(() => {
    if (spaceId === null) return;
    let stopped = false;
    let socket: WebSocket | null = null;
    let debounce: ReturnType<typeof setTimeout> | undefined;
    let reconnect: ReturnType<typeof setTimeout> | undefined;
    let delay = RECONNECT_FIRST_MS;
    /** Quand la première trame non traitée est arrivée ; `null` : aucune en attente. */
    let waitingSince: number | null = null;
    /** La première trame de la connexion n'a pas encore été vue. */
    let first = true;
    /** La première trame diffère de la version de l'arbre : relire, même plus bas. */
    let forced = false;
    /** La plus haute version reçue sur ce canal. */
    let seen = -1;

    const fire = () => {
      debounce = undefined;
      waitingSince = null;
      const current = callbacks.current.known();
      const due = forced || current === null || seen > current;
      forced = false;
      if (due) callbacks.current.onChange(seen);
    };

    const connect = () => {
      reconnect = undefined;
      const mine = new WebSocket(eventsUrl(spaceId));
      socket = mine;
      first = true;
      mine.onopen = () => {
        if (!stopped && socket === mine) setLive(true);
      };
      mine.onmessage = (event: MessageEvent) => {
        if (stopped || socket !== mine) return;
        const lsn = lsnOf(event.data);
        if (lsn === null) return;
        delay = RECONNECT_FIRST_MS;
        const current = callbacks.current.known();
        const opening = first;
        first = false;
        // Une version plus basse que celle déjà vue sur cette connexion ne compte pas, sauf
        // à l'ouverture : `seen` part de ce que dit la première trame.
        if (opening || lsn > seen) seen = lsn;
        if (opening && current !== null && lsn !== current) forced = true;
        // Pas plus récente que l'arbre : rien à relire, et pas de minuteur.
        if (!forced && current !== null && lsn <= current) return;
        const now = Date.now();
        waitingSince ??= now;
        clearTimeout(debounce);
        debounce = setTimeout(
          fire,
          Math.max(0, Math.min(EVENTS_DEBOUNCE_MS, waitingSince + EVENTS_MAX_WAIT_MS - now)),
        );
      };
      mine.onclose = () => {
        if (stopped || socket !== mine) return;
        socket = null;
        setLive(false);
        reconnect = setTimeout(connect, delay);
        delay = Math.min(delay * 2, RECONNECT_MAX_MS);
      };
    };
    connect();

    return () => {
      stopped = true;
      clearTimeout(debounce);
      clearTimeout(reconnect);
      if (socket !== null) {
        socket.onopen = socket.onmessage = socket.onclose = null;
        socket.close();
      }
      setLive(false);
    };
  }, [spaceId]);

  return { live };
}
