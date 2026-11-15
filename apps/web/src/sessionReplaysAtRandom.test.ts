// Les rejeux d'`api.ts` au hasard (Rulings R347, R348) : ce que les exemples d'`api.test.ts`
// affirment un par un, éprouvé sur des suites d'événements tirées au hasard. L'onglet a chargé
// l'application sous Alice ; il envoie des écritures, se connecte (sous Alice ou sous Bruno),
// change son mot de passe, se déconnecte, relit son compte ; ailleurs, un autre onglet du même
// navigateur se connecte (le cookie est commun), la session est perdue, les cookies effacés ; et les
// réponses reviennent dans n'importe quel ordre. Le serveur du test tient les sessions et le cookie
// comme le vrai : un appel part avec le cookie du moment, et le serveur juge la session quand il
// répond.
//
// Ce qui doit toujours tenir :
// - un appel repart au plus une fois ;
// - jamais une route de session ;
// - jamais sous un autre compte que celui de l'application (Alice), qui l'a envoyé : le rejeu
//   porte le cookie d'une session d'Alice, ou d'aucune ; sauf dans la fenêtre dite plus bas ;
// - jamais s'il est parti avant que l'onglet commence à se déconnecter.
//
// La fenêtre (R348, coût si faux). Le rejeu ne part que si la relecture du compte qui le précède a
// dit Alice ; mais cette relecture part avec le cookie du moment, et le rejeu avec celui de sa
// réponse. Si un autre onglet se connecte sous Bruno pendant la relecture, le rejeu part avec le
// cookie de Bruno : la page ne lit pas son cookie (`HttpOnly`), seul le serveur pourrait l'empêcher.
// L'essai l'affirme donc là où il se peut : un rejeu qui porte le cookie qu'a porté sa relecture
// est sous Alice, toujours ; un rejeu qui porte un autre cookie est compté (`window`), pas
// reproché. Le modèle ne joue un événement qu'entre deux tours de la boucle (`flush`), comme le
// navigateur entre deux réponses du réseau : le rejeu part dans le tour de la réponse de sa
// relecture.
//
// Trouvé par cet essai, et corrigé : un appel parti quand l'onglet avait appris que la session
// était à Bruno (graine 169), daté de Bruno, repartait sous Bruno (`acting`, `api.ts`).
//
// `MEMENTUM_FUZZ_SEEDS=5000` (et `MEMENTUM_FUZZ_STEPS`) élargit l'essai.
import type { Me } from "@mementum/protocol";
import { afterEach, expect, test, vi } from "vitest";
import { page } from "./test/pages";

const SEEDS = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 1000);
const STEPS = Number(import.meta.env.MEMENTUM_FUZZ_STEPS ?? 40);

const ALICE = "alice";
const BRUNO = "bruno";
const SESSION_PATHS = new Set([
  "/api/v1/auth/login",
  "/api/v1/auth/signup",
  "/api/v1/auth/logout",
  "/api/v1/auth/password",
]);
const UNAUTHENTICATED = { error: "unauthenticated", message: "session absente ou échue" };

/** Des nombres au hasard, les mêmes à chaque exécution pour une même graine (mulberry32). */
function random(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

const me = (id: string): Me => ({
  user: { id, email: `${id}@exemple.fr`, name: id, instanceAdmin: false },
  spaces: [],
});

/** Une réponse, réduite à ce qu'`api.ts` en lit : tout se passe en microtâches. */
function response(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => {
      if (body === undefined) throw new SyntaxError("pas de corps");
      return body;
    },
  } as Response;
}

/** Un appel en route : parti avec le cookie `token`, il attend que le serveur du test réponde. */
type InFlight = {
  method: string;
  path: string;
  body: Record<string, unknown> | undefined;
  token: number | null;
  /** Une relecture du compte faite par `api.ts` avant un rejeu, pas par l'essai. */
  verification: boolean;
  respond(): void;
};

/** Ce que l'essai a vu passer, toutes graines confondues : il n'est pas vide de sens. */
const seen = { replays: 0, refusedOtherAccount: 0, sessionCalls: 0, window: 0 };

afterEach(() => {
  vi.unstubAllGlobals();
});

/**
 * Laisse les promesses suivre jusqu'au bout, comme entre deux événements du réseau : une tâche,
 * après toutes les microtâches. Par `setImmediate` de Node, que l'environnement jsdom de Vitest
 * garde : sous Windows, `setTimeout(0)` attend la granularité de l'horloge (une quinzaine de
 * millisecondes), et l'essai durait quarante secondes.
 */
const nextTask = (globalThis as { setImmediate?: (run: () => void) => void }).setImmediate;
const flush = () =>
  new Promise<void>((done) => (nextTask ? nextTask(done) : setTimeout(done, 0)));

async function play(seed: number): Promise<void> {
  const next = random(seed * 7919 + 17);
  const pick = <T>(items: readonly T[]): T => items[Math.floor(next() * items.length)];
  const journal: string[] = [];
  const fail = (what: string) => {
    throw new Error(`graine ${seed} : ${what}\n${journal.join("\n")}`);
  };
  /**
   * Ce qui n'aurait pas dû partir, relevé par `fetch` : lever là ferait de l'envoi une panne du
   * réseau, qu'`api.ts` rendrait à l'appel, et que l'essai ne verrait pas.
   */
  const breaches: string[] = [];
  const breach = (what: string) => {
    breaches.push(what);
    journal.push(`!! ${what}`);
  };

  // Le serveur : les sessions (jeton → compte, vivante ou non), le cookie du navigateur.
  const sessions = new Map<number, { owner: string; alive: boolean }>();
  let tokens = 0;
  let cookie: number | null = null;
  const open = (owner: string) => {
    tokens += 1;
    sessions.set(tokens, { owner, alive: true });
    return tokens;
  };
  const close = (token: number | null) => {
    if (token !== null) sessions.get(token)!.alive = false;
  };
  const ownerOf = (token: number | null) => (token === null ? null : sessions.get(token)!.owner);
  const alive = (token: number | null) => token !== null && sessions.get(token)!.alive;

  const inFlight: InFlight[] = [];
  /** Les écritures de l'application : sous quel compte elles sont parties, et combien de fois. */
  const writes = new Map<string, { sends: number; logoutsAtSend: number }>();
  /** Les déconnexions que l'onglet a commencées. */
  let logouts = 0;
  /** Les appels des routes de session faits par l'onglet, et ceux que le serveur a reçus. */
  let sessionCalls = 0;
  let sessionFetches = 0;
  /** L'essai relit lui-même le compte : l'appel qui part n'est pas une relecture d'`api.ts`. */
  let reading = false;
  /** Le cookie qu'a porté la dernière relecture d'`api.ts` à laquelle le serveur a répondu. */
  let readWith: number | null | undefined;

  vi.stubGlobal("fetch", (path: string, init: RequestInit = {}) => {
    const method = init.method ?? "GET";
    const body =
      typeof init.body === "string" ? (JSON.parse(init.body) as Record<string, unknown>) : undefined;
    const token = cookie;
    journal.push(`→ ${method} ${path} ${body ? JSON.stringify(body) : ""} (cookie ${token})`);
    if (SESSION_PATHS.has(path)) sessionFetches += 1;
    if (method === "PATCH") {
      const key = String(body?.title);
      const write = writes.get(key)!;
      write.sends += 1;
      if (write.sends > 1) {
        seen.replays += 1;
        if (write.sends > 2) breach(`${key} repart une deuxième fois`);
        if (logouts !== write.logoutsAtSend) breach(`${key}, parti avant une déconnexion, repart`);
        const owner = ownerOf(token);
        if (owner !== null && owner !== ALICE) {
          // La relecture qui l'a permis portait un autre cookie : la fenêtre de l'en-tête.
          if (token === readWith) breach(`${key}, parti sous Alice, repart sous ${owner}`);
          else seen.window += 1;
        }
      }
    }
    const verification = method === "GET" && path === "/api/v1/me" && !reading;
    return new Promise<Response>((resolve) => {
      inFlight.push({
        method,
        path,
        body,
        token,
        verification,
        respond() {
          // Le rejeu qu'elle permet part dans ce tour, avant tout autre événement.
          if (verification) readWith = token;
          // Le serveur juge la session quand il répond ; un cookie neuf est posé avant que la page
          // lise la réponse.
          if (path === "/api/v1/auth/login") {
            const account = String(body?.email).split("@")[0];
            close(token);
            cookie = open(account);
            return resolve(response(200, me(account)));
          }
          if (path === "/api/v1/auth/logout") {
            close(token);
            cookie = null;
            return resolve(response(204));
          }
          if (!alive(token)) return resolve(response(401, UNAUTHENTICATED));
          if (path === "/api/v1/auth/password") {
            const owner = ownerOf(token)!;
            for (const session of sessions.values()) if (session.owner === owner) session.alive = false;
            cookie = open(owner);
            return resolve(response(204));
          }
          if (path === "/api/v1/me") {
            const owner = ownerOf(token)!;
            if (verification && owner !== ALICE) seen.refusedOtherAccount += 1;
            return resolve(response(200, me(owner)));
          }
          return resolve(response(200, page("p")));
        },
      });
    });
  });

  vi.resetModules();
  const api = await import("./api");

  // L'application se charge sous Alice.
  cookie = open(ALICE);
  reading = true;
  const loading = api.getMe();
  reading = false;
  await flush();
  inFlight.shift()!.respond();
  await loading;

  /** L'application tourne encore : elle n'a vu ni un autre compte, ni la déconnexion. */
  let running = true;
  /** Une connexion, une déconnexion ou un changement de mot de passe de l'onglet est en route. */
  let changing = false;
  let written = 0;

  const sessionCall = (name: string, run: () => Promise<unknown>) => {
    sessionCalls += 1;
    changing = true;
    journal.push(`onglet : ${name}`);
    void run()
      .catch(() => {})
      .finally(() => (changing = false));
  };

  const steps: [number, () => void][] = [
    [
      30,
      () => {
        if (inFlight.length === 0) return;
        const call = inFlight.splice(Math.floor(next() * inFlight.length), 1)[0];
        journal.push(`← ${call.method} ${call.path}`);
        call.respond();
      },
    ],
    [
      20,
      () => {
        if (!running) return;
        written += 1;
        const key = `w${written}`;
        writes.set(key, { sends: 0, logoutsAtSend: logouts });
        void api.updatePage("p", { title: key }).catch(() => {});
      },
    ],
    [
      10,
      () => {
        if (!running || changing) return;
        const account = pick([ALICE, ALICE, BRUNO]);
        sessionCall(`connexion de ${account}`, async () => {
          const opened = await api.logIn({ email: `${account}@exemple.fr`, password: "x" });
          // Sous un autre compte, l'application repart de zéro (R115).
          if (opened.user.id !== ALICE) running = false;
        });
      },
    ],
    [
      6,
      () => {
        if (!running || changing) return;
        sessionCall("changement du mot de passe", () =>
          api.changePassword({ current: "x", new: "y" }),
        );
      },
    ],
    [
      3,
      () => {
        if (!running || changing) return;
        // L'application se déconnecte, puis recharge l'onglet : elle n'écrit plus rien.
        logouts += 1;
        running = false;
        sessionCall("déconnexion", () => api.logOut());
      },
    ],
    [
      5,
      () => {
        if (!running) return;
        journal.push("onglet : relit le compte");
        reading = true;
        const read = api.getMe();
        reading = false;
        void read
          .then((found) => {
            if (found.user.id !== ALICE) running = false;
          })
          .catch(() => {});
      },
    ],
    [
      8,
      () => {
        const account = pick([ALICE, BRUNO]);
        journal.push(`autre onglet : connexion de ${account}`);
        close(cookie);
        cookie = open(account);
      },
    ],
    [
      8,
      () => {
        journal.push("la session est perdue (déconnexion ailleurs, échéance)");
        close(cookie);
      },
    ],
    [
      3,
      () => {
        journal.push("cookies effacés");
        cookie = null;
      },
    ],
  ];
  const total = steps.reduce((sum, [weight]) => sum + weight, 0);

  for (let step = 0; step < STEPS; step++) {
    let roll = next() * total;
    for (const [weight, act] of steps) {
      roll -= weight;
      if (roll < 0) {
        act();
        break;
      }
    }
    await flush();
  }
  // Tout ce qui est en route revient, rejeux compris.
  for (let round = 0; inFlight.length > 0; round++) {
    if (round > 500) fail("des appels restent en route");
    const call = inFlight.splice(Math.floor(next() * inFlight.length), 1)[0];
    journal.push(`← ${call.method} ${call.path}`);
    call.respond();
    await flush();
  }
  if (breaches.length > 0) fail(breaches.join(" ; "));
  if (sessionFetches !== sessionCalls) {
    fail(`${sessionFetches} envois de routes de session pour ${sessionCalls} appels`);
  }
  seen.sessionCalls += sessionCalls;
}

const seeds = Array.from({ length: SEEDS }, (_, i) => i + 1);

test.each(seeds)("graine %i", async (seed) => {
  await play(seed);
});

test("the_random_runs_did_replay_and_did_refuse_another_account", () => {
  // Les graines ont tourné avant (même fichier, dans l'ordre) : l'essai a bien vu des rejeux, et
  // des relectures du compte qui disaient un autre compte.
  expect(seen.replays).toBeGreaterThan(0);
  expect(seen.refusedOtherAccount).toBeGreaterThan(0);
  expect(seen.sessionCalls).toBeGreaterThan(0);
});
