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
//
// Un canal peut aussi mourir sans bruit (connexion à demi ouverte : aucun événement de
// fermeture, et le navigateur ne montre pas les pings du serveur). La barre latérale resterait
// alors périmée sans limite, son minuteur arrêté. Le serveur redit donc la version à chaque
// ping, et un chien de garde, armé à l'ouverture et remis à zéro par chaque trame, au numéro égal
// compris, ferme un canal resté muet `EVENTS_SILENCE_MS`, sans attendre sa fermeture : `live`
// passe à faux, et la reconnexion habituelle repart. Le retour du réseau (`online`) rouvre le
// canal tout de suite, sans attendre le délai de reconnexion, et remplace celui qui était tenu
// pour ouvert : il a pu mourir pendant la coupure.
import { useEffect, useRef, useState } from "react";
import { groupedDelay, REREAD_DEBOUNCE_MS, REREAD_MAX_WAIT_MS } from "../rereads";

/** Plusieurs trames qui se suivent ne font qu'une relecture, cette durée après la dernière. */
export const EVENTS_DEBOUNCE_MS = REREAD_DEBOUNCE_MS;
/**
 * Une relecture part au plus tard cette durée après la première trame qu'elle doit traiter :
 * une suite de trames plus rapprochées que l'anti-rebond ne fige pas la barre latérale. Le
 * calcul est celui des relectures groupées (`groupedDelay`, `rereads.ts`).
 */
export const EVENTS_MAX_WAIT_MS = REREAD_MAX_WAIT_MS;
/** Délai avant la première reconnexion ; il double à chaque échec. */
export const RECONNECT_FIRST_MS = 1_000;
/** Le délai de reconnexion ne dépasse jamais cela. */
export const RECONNECT_MAX_MS = 30_000;
/**
 * Un canal ouvert dont rien n'est arrivé depuis cette durée est tenu pour mort, et fermé. Le
 * serveur redit `{"lsn":N}` à chaque ping, toutes les 20 s (`EVENTS_PING`,
 * `crates/mementum-server/src/events.rs`) : 45 s laissent passer deux pings et une marge. Plus
 * court que deux intervalles du serveur, un canal sain serait fermé à tort.
 */
export const EVENTS_SILENCE_MS = 45_000;

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
 * Ouvre le canal de l'espace `spaceId` (`null` : aucun). `onChange(lsn, forced)` est appelé,
 * avec la plus haute version reçue, quand elle dépasse `known()`, la version de l'arbre affiché
 * (`null` : pas d'arbre encore ; c'est à l'appelant de ne pas jeter la lecture en route).
 * `forced` : la première trame de la connexion diffère de cette version, plus basse comprise
 * (R156) : l'arbre est à relire quelle que soit la version qu'il rend ensuite.
 * `live` : le canal est ouvert.
 */
export function useSpaceEvents(
  spaceId: string | null,
  onChange: (lsn: number, forced: boolean) => void,
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
    /** Le chien de garde du canal ouvert : tombe s'il ne reçoit rien. */
    let silence: ReturnType<typeof setTimeout> | undefined;
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
      const wasForced = forced;
      forced = false;
      if (due) callbacks.current.onChange(seen, wasForced);
    };

    /** Le canal `mine` est tombé : `live` à faux, et la reconnexion après le délai. */
    const down = (mine: WebSocket) => {
      if (stopped || socket !== mine) return;
      clearTimeout(silence);
      silence = undefined;
      socket = null;
      setLive(false);
      reconnect = setTimeout(connect, delay);
      delay = Math.min(delay * 2, RECONNECT_MAX_MS);
    };

    /** Ferme `mine` sans attendre son événement de fermeture, et le fait taire. */
    const drop = (mine: WebSocket) => {
      mine.onopen = mine.onmessage = mine.onclose = null;
      mine.close();
    };

    /** (Ré)arme le chien de garde de `mine`, ouvert. */
    const watch = (mine: WebSocket) => {
      clearTimeout(silence);
      silence = setTimeout(() => {
        silence = undefined;
        if (stopped || socket !== mine) return;
        down(mine);
        drop(mine);
      }, EVENTS_SILENCE_MS);
    };

    const connect = () => {
      reconnect = undefined;
      const mine = new WebSocket(eventsUrl(spaceId));
      socket = mine;
      first = true;
      mine.onopen = () => {
        if (stopped || socket !== mine) return;
        setLive(true);
        watch(mine);
      };
      mine.onmessage = (event: MessageEvent) => {
        if (stopped || socket !== mine) return;
        // Toute trame dit que le canal vit, au numéro égal compris.
        watch(mine);
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
        debounce = setTimeout(fire, groupedDelay(waitingSince, now));
      };
      mine.onclose = () => down(mine);
    };
    connect();

    // Le réseau revient : le canal est rouvert tout de suite, et celui qui était tenu pour
    // ouvert est remplacé (il a pu mourir pendant la coupure sans rien dire).
    const onOnline = () => {
      if (stopped) return;
      clearTimeout(reconnect);
      clearTimeout(silence);
      silence = undefined;
      if (socket !== null) {
        drop(socket);
        setLive(false);
      }
      socket = null;
      delay = RECONNECT_FIRST_MS;
      connect();
    };
    window.addEventListener("online", onOnline);

    return () => {
      stopped = true;
      window.removeEventListener("online", onOnline);
      clearTimeout(debounce);
      clearTimeout(reconnect);
      clearTimeout(silence);
      if (socket !== null) drop(socket);
      setLive(false);
    };
  }, [spaceId]);

  return { live };
}
