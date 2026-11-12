// Les relectures groupées : relire une ressource par l'API quand le serveur dit qu'elle a changé
// (une trame de page, la version de l'espace), sans relire à chaque signal. Les fils de
// commentaires (`useComments`) et les versions d'une page (`useVersions`) s'en servent ; le canal
// de l'espace (`useSpaceEvents`) en prend le calcul d'attente.
//
// Les règles, une seule fois ici :
// - une rafale de demandes ne fait qu'une lecture, `REREAD_DEBOUNCE_MS` après la dernière, et au
//   plus tard `REREAD_MAX_WAIT_MS` après la première (`groupedDelay`) ;
// - une demande immédiate part au tour suivant, et une demande groupée ne la repousse pas ;
// - une lecture en route n'est jamais annulée : une demande faite pendant elle est servie une
//   fois, à sa fin ; la lecture elle-même peut demander à être refaite (sa réponse peut dater
//   d'avant une écriture d'ici), en rendant `true` ;
// - une lecture qui échoue ne se refait pas d'elle-même : c'est à la demande suivante ;
// - après `stop`, plus rien ne part, ni demande, ni relecture due.
//
// La lecture traite elle-même ses erreurs et ses réponses ; après `stop`, elle n'en fait rien
// (`stopped`).

/** Plusieurs demandes qui se suivent ne font qu'une lecture, cette durée après la dernière (ms). */
export const REREAD_DEBOUNCE_MS = 250;
/**
 * Une lecture part au plus tard cette durée après la première demande qu'elle doit traiter (ms) :
 * une suite de demandes plus rapprochées que l'anti-rebond ne fige pas l'écran.
 */
export const REREAD_MAX_WAIT_MS = 1_000;

/**
 * Le délai d'une lecture groupée demandée à `now`, la première demande non traitée étant
 * arrivée à `waitingSince` (millisecondes) : l'anti-rebond, borné par l'attente maximale.
 */
export function groupedDelay(waitingSince: number, now: number): number {
  return Math.max(0, Math.min(REREAD_DEBOUNCE_MS, waitingSince + REREAD_MAX_WAIT_MS - now));
}

export type Rereads = {
  /** Demande une lecture : tout de suite (`true`, au tour suivant), ou groupée. */
  request(immediate: boolean): void;
  /** Plus rien ne part ; une lecture en route finit sans suite. */
  stop(): void;
  /** `stop` a été appelé : la lecture en route ne fait rien de sa réponse. */
  readonly stopped: boolean;
};

/** Ordonne les lectures `read` (voir l'en-tête) ; `read` rend `true` pour être refaite. */
export function scheduleRereads(read: () => Promise<boolean | void>): Rereads {
  let stopped = false;
  /** Une lecture est en route. */
  let reading = false;
  /** Une lecture a été demandée pendant la lecture en route : elle sera faite à sa fin. */
  let due = false;
  /** Une lecture immédiate est prévue : une demande groupée ne la repousse pas. */
  let urgent = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Quand la première demande non traitée est arrivée ; `null` : aucune en attente. */
  let waitingSince: number | null = null;

  const run = async () => {
    clearTimeout(timer);
    timer = undefined;
    waitingSince = null;
    urgent = false;
    reading = true;
    due = false;
    try {
      if ((await read()) === true) due = true;
    } catch {
      // La lecture dit elle-même ce qui a échoué ; rien ne se refait sans une demande.
    } finally {
      reading = false;
      if (!stopped && due) void run();
    }
  };

  return {
    request(immediate) {
      if (stopped) return;
      if (reading) {
        due = true;
        return;
      }
      if (immediate) {
        clearTimeout(timer);
        urgent = true;
        timer = setTimeout(() => void run(), 0);
        return;
      }
      if (urgent) return;
      const now = Date.now();
      waitingSince ??= now;
      clearTimeout(timer);
      timer = setTimeout(() => void run(), groupedDelay(waitingSince, now));
    },
    stop() {
      stopped = true;
      clearTimeout(timer);
    },
    get stopped() {
      return stopped;
    },
  };
}
