// Les rejeux d'`api.ts` au hasard (Rulings R347, R348, R460 à R464, R480, R540, R543) : ce que les
// exemples d'`api.test.ts` affirment un par un, éprouvé sur des suites d'événements tirées au hasard.
//
// Le modèle, rendu fidèle au tour de correction 1 (constat I2 de la relecture). L'onglet a chargé
// l'application sous Alice ; il écrit, se connecte (sous Alice ou sous Bruno), change son mot de
// passe, se déconnecte, relit son compte, et ces sessions peuvent se croiser (deux formulaires) ;
// l'application reste montée pendant une déconnexion, et après une déconnexion qui échoue. Ailleurs,
// un autre onglet du même navigateur se connecte (le cookie est commun), la session est perdue, les
// cookies sont effacés. Chaque appel passe par trois temps, que l'essai mène un par un et dans
// n'importe quel ordre d'un appel à l'autre : le serveur le traite (il juge la session, l'ouvre ou
// la ferme), puis ses en-têtes arrivent (le navigateur pose le cookie), puis la page lit la
// réponse. Une réponse peut aussi se perdre une fois l'appel traité : avant ses en-têtes, `fetch`
// rejette ; après eux, comme dans un navigateur, le cookie est posé, `fetch` est résolu, et c'est la
// lecture du corps qui échoue (re-relecture, N1). Rien n'est donc atomique : « le serveur a changé
// la session, la page ne le sait pas encore » se produit.
//
// Deux régimes. Une graine sur deux est agitée (autres onglets, sessions perdues, déconnexions,
// sessions de l'onglet qui se croisent) ; l'autre est calme, et les enchaînements longs s'y déroulent
// jusqu'au bout. Deux pas sont menés plus souvent que le hasard seul ne le ferait : une connexion de
// l'onglet pendant une relecture du compte d'`api.ts`, dont le corps se perd une fois sur deux après
// ses en-têtes, et la perte du corps d'une session de l'onglet. Sans eux et sans le régime calme,
// retirer R480 survivait à 30 000 graines ; il échoue maintenant quelques graines sur 30 000.
//
// Le serveur du modèle vérifie aussi le compte attendu (R540) : un appel qui nomme un compte
// (`X-Mementum-Account`) autre que celui de la session est refusé par `409 account_changed`, sans
// rien faire, juste après le `401`.
//
// Ce qui doit toujours tenir :
// - un appel repart au plus une fois ;
// - jamais une route de session ;
// - jamais s'il est parti avant que l'onglet commence à se déconnecter, ni pendant ;
// - chaque écriture de l'application nomme Alice, son compte, au premier envoi comme au rejeu ;
// - les gardes du client tiennent toujours (R460 à R462, R480) : un rejeu ne part avec le cookie
//   d'une session de Bruno que dans la fenêtre de R460, une session qu'un autre onglet a ouverte
//   après le départ de la dernière relecture du compte lue (retirer R480 fait encore échouer 10
//   graines sur 30 000 : « parti sous Alice, repart sous bruno ») ;
// - aucune écriture n'est jamais faite sous un autre compte qu'Alice, rejeu ou non. Avant l'en-tête,
//   un rejeu partait sous Bruno dans cette fenêtre (que l'essai comptait à part, sans la
//   reprocher), et tout appel parti avant que
//   l'application apprenne l'autre compte était fait sous lui (R115) : le serveur les refuse
//   désormais (comptés, `refusedByHeader` ; les rejeux parmi eux, `replaysRefusedByHeader`). La
//   fenêtre de R460 est menée exprès : une relecture traitée sous Alice, hors d'une session de
//   l'onglet en route, laisse un autre onglet se connecter sous Bruno avant que la page lise sa
//   réponse (une fois sur deux dans les graines calmes, une sur quatre ailleurs ; `windowSteps`) ;
//   sans cela, l'essai ne l'atteignait jamais. Si le rejeu perdait l'en-tête, 97 graines sur 1 000
//   échoueraient (« wN fait sous bruno »).
// Le modèle ne joue un événement qu'entre deux tours de la boucle (`flush`), comme le navigateur
// entre deux réponses du réseau.
//
// Trouvé par cet essai, et corrigé : un appel parti quand l'onglet avait appris que la session était
// à Bruno, daté de Bruno, repartait sous Bruno (graine 169 du premier modèle ; `acting`) ; une
// relecture répondue pendant une déconnexion rechargeait le compte de l'application (relecture, I1 ;
// R461) ; une connexion de Bruno dont la réponse se perdait après son cookie laissait repartir un
// appel sous lui (R480, que la forme réelle de la perte, le corps après les en-têtes, met en défaut
// sans la correction N1) ; un changement de mot de passe refusé par `409 account_changed` attendait
// la relecture du compte, qui attendait elle-même l'issue de ce changement : dix secondes de refus
// bloqués (graine 14934 à 30 000 graines, par le minuteur d'une graine précédente ; R543).
//
// `MEMENTUM_FUZZ_SEEDS=30000` (et `MEMENTUM_FUZZ_STEPS`) élargit l'essai.
import type { Me } from "@mementum/protocol";
import { afterEach, expect, test, vi } from "vitest";
import { page } from "./test/pages";

const SEEDS = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 1000);
const STEPS = Number(import.meta.env.MEMENTUM_FUZZ_STEPS ?? 60);

const ALICE = "alice";
const BRUNO = "bruno";
const SESSION_PATHS = new Set([
  "/api/v1/auth/login",
  "/api/v1/auth/signup",
  "/api/v1/auth/logout",
  "/api/v1/auth/password",
]);
const UNAUTHENTICATED = { error: "unauthenticated", message: "session absente ou échue" };
const ACCOUNT_CHANGED = { error: "account_changed", message: "rien n'a été fait" };

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

/** Une session du serveur : son compte, si elle vaut encore, qui l'a ouverte, et quand. */
type Session = { owner: string; alive: boolean; origin: "load" | "tab" | "other"; openedAt: number };

/** Un appel en route : parti avec le cookie `token`, il passe par les trois temps de l'en-tête. */
type InFlight = {
  method: string;
  path: string;
  body: Record<string, unknown> | undefined;
  token: number | null;
  /** Le compte que l'appel nomme (`X-Mementum-Account`) ; `null` : aucun. */
  named: string | null;
  /** Une relecture du compte faite par `api.ts` avant un rejeu, pas par l'essai. */
  verification: boolean;
  /** Le pas de l'essai où il est parti. */
  sentAt: number;
  /** 0 : en route ; 1 : traité par le serveur ; 2 : en-têtes reçus. */
  phase: 0 | 1 | 2;
  /** Ce que le serveur a répondu, et le cookie que posent ses en-têtes (`undefined` : aucun). */
  outcome?: { status: number; body?: unknown; setCookie?: number | null };
  /** Son corps se perdra après ses en-têtes : quand vient sa lecture, il est perdu. */
  losesBody: boolean;
  resolve(res: Response): void;
  reject(error: unknown): void;
};

/** Ce que l'essai a vu passer, toutes graines confondues : il n'est pas vide de sens. */
const seen = {
  replays: 0,
  refusedOtherAccount: 0,
  sessionCalls: 0,
  refusedByHeader: 0,
  replaysRefusedByHeader: 0,
  windowSteps: 0,
  writesDuringLogout: 0,
  lostAnswers: 0,
  lostBodies: 0,
  crossedSessions: 0,
  loginsDuringReading: 0,
};

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

  /** Le pas de l'essai. */
  let tick = 0;
  /** Combien de fois sur une, une relecture traitée sous Alice ouvre la fenêtre de R460. */
  const windowRate = seed % 2 === 0 ? 0.5 : 0.25;
  // Le serveur : les sessions (jeton → session), le cookie du navigateur.
  const sessions = new Map<number, Session>();
  let tokens = 0;
  let cookie: number | null = null;
  const open = (owner: string, origin: Session["origin"]) => {
    tokens += 1;
    sessions.set(tokens, { owner, alive: true, origin, openedAt: tick });
    journal.push(`⇢ serveur : session ${tokens} ouverte, ${owner} (${origin})`);
    return tokens;
  };
  const close = (token: number | null) => {
    if (token !== null) sessions.get(token)!.alive = false;
  };
  const ownerOf = (token: number | null) => (token === null ? null : sessions.get(token)!.owner);
  const alive = (token: number | null) => token !== null && sessions.get(token)!.alive;

  const inFlight: InFlight[] = [];
  /** Les écritures de l'application : quand elles sont parties, et combien de fois. */
  const writes = new Map<string, { sends: number; logoutsAtSend: number; duringLogout: boolean }>();
  /** Les déconnexions que l'onglet a commencées, et celles en route. */
  let logouts = 0;
  let loggingOut = 0;
  /** Les appels des routes de session faits par l'onglet, et ceux que le serveur a reçus. */
  let sessionCalls = 0;
  let sessionFetches = 0;
  /** Les sessions de l'onglet en route. */
  let changing = 0;
  /** L'essai relit lui-même le compte : l'appel qui part n'est pas une relecture d'`api.ts`. */
  let reading = false;
  /** Le pas où est partie la dernière relecture d'`api.ts` dont la page a lu la réponse. */
  let lastReadingSentAt: number | undefined;
  /** L'appel qui part maintenant perdra son corps après ses en-têtes. */
  let nextLosesBody = false;

  vi.stubGlobal("fetch", (path: string, init: RequestInit = {}) => {
    const method = init.method ?? "GET";
    const body =
      typeof init.body === "string" ? (JSON.parse(init.body) as Record<string, unknown>) : undefined;
    const token = cookie;
    const named = new Headers(init.headers).get("x-mementum-account");
    journal.push(
      `→ ${method} ${path} ${body ? JSON.stringify(body) : ""} (cookie ${token}, nomme ${named})`,
    );
    if (SESSION_PATHS.has(path)) sessionFetches += 1;
    if (method === "PATCH") {
      const key = String(body?.title);
      const write = writes.get(key)!;
      write.sends += 1;
      if (named !== ALICE) breach(`${key} nomme ${named}, pas Alice`);
      if (write.sends > 1) {
        seen.replays += 1;
        if (write.sends > 2) breach(`${key} repart une deuxième fois`);
        if (logouts !== write.logoutsAtSend) breach(`${key}, parti avant une déconnexion, repart`);
        if (write.duringLogout) breach(`${key}, parti pendant une déconnexion, repart`);
        // Les gardes du client (R460 à R462, R480) : un rejeu ne part avec le cookie d'une session
        // de Bruno que dans la fenêtre, une session qu'un autre onglet a ouverte après le départ
        // de la dernière relecture lue ; le serveur doit alors le refuser (plus bas, `process`).
        const session = token === null ? null : sessions.get(token)!;
        if (session && session.owner !== ALICE) {
          const window =
            session.origin === "other" &&
            lastReadingSentAt !== undefined &&
            session.openedAt > lastReadingSentAt;
          if (!window) {
            breach(`${key}, parti sous Alice, repart sous ${session.owner} (session ${token})`);
          }
        }
      }
    }
    const verification = method === "GET" && path === "/api/v1/me" && !reading;
    return new Promise<Response>((resolve, reject) => {
      inFlight.push({
        method,
        path,
        body,
        token,
        named,
        verification,
        sentAt: tick,
        phase: 0,
        losesBody: nextLosesBody,
        resolve,
        reject,
      });
    });
  });

  /** Le serveur traite l'appel : il juge la session, l'ouvre ou la ferme, et dit sa réponse. */
  const process = (call: InFlight) => {
    const { path, token } = call;
    journal.push(`⇢ serveur : ${call.method} ${path}`);
    call.phase = 1;
    if (path === "/api/v1/auth/login") {
      const account = String(call.body?.email).split("@")[0];
      close(token);
      const opened = open(account, "tab");
      call.outcome = { status: 200, body: me(account), setCookie: opened };
    } else if (path === "/api/v1/auth/logout") {
      close(token);
      call.outcome = { status: 204, setCookie: null };
    } else if (!alive(token)) {
      call.outcome = { status: 401, body: UNAUTHENTICATED };
    } else if (call.named !== null && call.named !== ownerOf(token)) {
      // Le compte attendu (R540) : rien n'est fait.
      seen.refusedByHeader += 1;
      if (call.method === "PATCH" && writes.get(String(call.body?.title))!.sends > 1) {
        seen.replaysRefusedByHeader += 1;
      }
      call.outcome = { status: 409, body: ACCOUNT_CHANGED };
    } else if (path === "/api/v1/auth/password") {
      const owner = ownerOf(token)!;
      for (const session of sessions.values()) if (session.owner === owner) session.alive = false;
      // Le corps dit les jetons d'API révoqués (R1365) : il peut se perdre après les en-têtes, et le
      // changement reste fait (R1375).
      call.outcome = { status: 200, body: { tokensRevoked: 0 }, setCookie: open(owner, "tab") };
    } else if (path === "/api/v1/me") {
      const owner = ownerOf(token)!;
      if (call.verification && owner !== ALICE) seen.refusedOtherAccount += 1;
      call.outcome = { status: 200, body: me(owner) };
      // La fenêtre de R460, menée exprès : une relecture d'`api.ts` vient d'être traitée sous la
      // session d'Alice, et un autre onglet se connecte sous Bruno avant que la page lise la
      // réponse. Le rejeu qu'elle permet part avec le cookie de Bruno ; le serveur le refuse
      // (R540). Hors d'une session de l'onglet en route, qui ferait relire le compte (R462).
      if (call.verification && owner === ALICE && changing === 0 && next() < windowRate) {
        seen.windowSteps += 1;
        journal.push("autre onglet : connexion de bruno, pendant la relecture");
        close(cookie);
        cookie = open(BRUNO, "other");
      }
    } else {
      if (call.method === "PATCH" && ownerOf(token) !== ALICE) {
        breach(`${String(call.body?.title)} fait sous ${ownerOf(token)} (session ${token})`);
      }
      call.outcome = { status: 200, body: page("p") };
    }
  };
  /** Les en-têtes arrivent : le navigateur pose le cookie. */
  const headers = (call: InFlight) => {
    journal.push(`⇢ en-têtes : ${call.method} ${call.path} (${call.outcome!.status})`);
    call.phase = 2;
    if (call.outcome!.setCookie !== undefined) cookie = call.outcome!.setCookie;
  };
  /** La page lit la réponse. */
  const deliver = (call: InFlight) => {
    inFlight.splice(inFlight.indexOf(call), 1);
    journal.push(`← ${call.method} ${call.path} (${call.outcome!.status})`);
    if (call.verification) lastReadingSentAt = call.sentAt;
    call.resolve(response(call.outcome!.status, call.outcome!.body));
  };
  /** Mène l'appel d'un temps. */
  const advance = (call: InFlight) => {
    if (call.phase === 0) process(call);
    else if (call.phase === 1) headers(call);
    else if (call.losesBody) lose(call);
    else deliver(call);
  };
  /**
   * La réponse de l'appel se perd, une fois l'appel traité par le serveur. Avant ses en-têtes,
   * `fetch` rejette, et le cookie n'est pas posé. Après eux, comme dans un navigateur, le cookie est
   * posé et `fetch` résolu ; c'est la lecture du corps qui échoue (re-relecture, N1). Une réponse
   * sans corps (`204`) est alors entière : elle est lue.
   */
  const lose = (call: InFlight) => {
    if (call.phase === 2 && call.outcome!.body === undefined) {
      deliver(call);
      return;
    }
    inFlight.splice(inFlight.indexOf(call), 1);
    seen.lostAnswers += 1;
    if (call.phase === 2) {
      journal.push(`✗ ${call.method} ${call.path} (corps perdu après les en-têtes)`);
      seen.lostBodies += 1;
      const { status } = call.outcome!;
      call.resolve({
        ok: status >= 200 && status < 300,
        status,
        json: () => Promise.reject(new TypeError("network error")),
      } as unknown as Response);
    } else {
      journal.push(`✗ ${call.method} ${call.path} (réponse perdue)`);
      call.reject(new TypeError("Failed to fetch"));
    }
  };
  const ofPhase = (phase: number) => inFlight.filter((call) => call.phase === phase);

  vi.resetModules();
  const api = await import("./api");

  /** L'application tourne encore : elle n'a vu ni un autre compte, ni sa déconnexion aboutir. */
  let running = true;
  // Prévenue qu'un autre compte a la session, l'application repart de zéro (R115, R464).
  api.onOtherAccount(() => {
    running = false;
  });

  // L'application se charge sous Alice.
  cookie = open(ALICE, "load");
  reading = true;
  const loading = api.getMe();
  reading = false;
  await flush();
  for (const call of [...inFlight]) {
    process(call);
    headers(call);
    deliver(call);
  }
  await loading;

  let written = 0;

  const sessionCall = (name: string, run: () => Promise<unknown>) => {
    sessionCalls += 1;
    if (changing > 0) seen.crossedSessions += 1;
    changing += 1;
    journal.push(`onglet : ${name}`);
    void run()
      .catch(() => {})
      .finally(() => (changing -= 1));
  };

  // Une graine sur deux est calme : peu d'autres onglets, de pertes et de déconnexions, et des
  // connexions de l'onglet pendant les relectures. Les enchaînements longs qu'y gardent R462 et R480
  // s'y déroulent jusqu'au bout ; dans les graines agitées, ils sont presque toujours dérangés.
  const calm = seed % 2 === 0;
  const steps: [number, () => void][] = [
    [
      14,
      () => {
        const calls = ofPhase(0);
        if (calls.length > 0) process(pick(calls));
      },
    ],
    [
      9,
      () => {
        const calls = ofPhase(1);
        if (calls.length > 0) headers(pick(calls));
      },
    ],
    [
      14,
      () => {
        const calls = ofPhase(2);
        if (calls.length === 0) return;
        const call = pick(calls);
        if (call.losesBody) lose(call);
        else deliver(call);
      },
    ],
    [
      calm ? 1 : 2,
      () => {
        // Une réponse se perd, l'appel une fois traité.
        const calls = inFlight.filter((call) => call.phase > 0);
        if (calls.length > 0) lose(pick(calls));
      },
    ],
    [
      calm ? 4 : 3,
      () => {
        // Une session de l'onglet perd son corps, ses en-têtes (son cookie) arrivés : ce qui rend
        // R480 nécessaire, mené plus souvent que le hasard seul ne le ferait.
        const calls = inFlight.filter((call) => call.phase === 2 && SESSION_PATHS.has(call.path));
        if (calls.length > 0) lose(pick(calls));
      },
    ],
    [
      calm ? 12 : 8,
      () => {
        // L'onglet se connecte pendant qu'une relecture d'`api.ts` est en route : la connexion
        // croise la relecture, ce que R462 et R480 gardent. Une fois sur deux, son corps se perdra
        // après ses en-têtes (son cookie posé) : la forme réelle d'une réponse perdue (N1).
        if (!running || !inFlight.some((call) => call.verification)) return;
        const account = pick([ALICE, BRUNO]);
        const losesBody = next() < 0.5;
        seen.loginsDuringReading += 1;
        sessionCall(`connexion de ${account}, pendant une relecture${losesBody ? ", corps perdu" : ""}`, async () => {
          nextLosesBody = losesBody;
          const login = api.logIn({ email: `${account}@exemple.fr`, password: "x" });
          nextLosesBody = false;
          const opened = await login;
          if (opened.user.id !== ALICE) running = false;
        });
      },
    ],
    [
      18,
      () => {
        if (!running) return;
        written += 1;
        const key = `w${written}`;
        writes.set(key, { sends: 0, logoutsAtSend: logouts, duringLogout: loggingOut > 0 });
        if (loggingOut > 0) seen.writesDuringLogout += 1;
        void api.updatePage("p", { title: key }).catch(() => {});
      },
    ],
    [
      calm ? 2 : 4,
      () => {
        if (!running) return;
        const account = pick([ALICE, ALICE, BRUNO]);
        sessionCall(`connexion de ${account}`, async () => {
          const opened = await api.logIn({ email: `${account}@exemple.fr`, password: "x" });
          // Sous un autre compte, l'application repart de zéro (R115).
          if (opened.user.id !== ALICE) running = false;
        });
      },
    ],
    [
      calm ? 4 : 3,
      () => {
        if (!running) return;
        sessionCall("changement du mot de passe", () =>
          api.changePassword({ current: "x", new: "y" }),
        );
      },
    ],
    [
      calm ? 0 : 2,
      () => {
        if (!running) return;
        // L'application reste montée jusqu'à la réponse, et après un échec (`SpaceSwitcher`).
        logouts += 1;
        loggingOut += 1;
        sessionCall("déconnexion", async () => {
          try {
            await api.logOut();
            running = false;
          } finally {
            loggingOut -= 1;
          }
        });
      },
    ],
    [
      calm ? 2 : 5,
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
      calm ? 1 : 7,
      () => {
        const account = pick([ALICE, BRUNO]);
        journal.push(`autre onglet : connexion de ${account}`);
        close(cookie);
        cookie = open(account, "other");
      },
    ],
    [
      calm ? 1 : 7,
      () => {
        journal.push("la session est perdue (déconnexion ailleurs, échéance)");
        close(cookie);
      },
    ],
    [
      calm ? 0 : 2,
      () => {
        journal.push("cookies effacés");
        cookie = null;
      },
    ],
  ];
  const total = steps.reduce((sum, [weight]) => sum + weight, 0);

  for (; tick < STEPS; tick++) {
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
  // Tout ce qui est en route revient, rejeux compris, chaque appel un temps à la fois.
  for (let round = 0; inFlight.length > 0; round++, tick++) {
    if (round > 2_000) fail("des appels restent en route");
    advance(pick(inFlight));
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

test("the_random_runs_did_meet_what_they_test", () => {
  // Les graines ont tourné avant (même fichier, dans l'ordre) : l'essai a bien vu des rejeux, des
  // relectures du compte qui disaient un autre compte, des écritures parties pendant une
  // déconnexion, des réponses perdues, des sessions de l'onglet qui se croisent, et des écritures que
  // le serveur a refusées parce qu'elles nommaient Alice sous la session de Bruno.
  if (import.meta.env.MEMENTUM_FUZZ_SEEDS) console.log("essai au hasard :", JSON.stringify(seen));
  expect(seen, JSON.stringify(seen)).toBeDefined();
  expect(seen.replays).toBeGreaterThan(0);
  expect(seen.refusedOtherAccount).toBeGreaterThan(0);
  expect(seen.sessionCalls).toBeGreaterThan(0);
  expect(seen.writesDuringLogout).toBeGreaterThan(0);
  expect(seen.lostAnswers).toBeGreaterThan(0);
  expect(seen.crossedSessions).toBeGreaterThan(0);
  expect(seen.lostBodies).toBeGreaterThan(0);
  expect(seen.loginsDuringReading).toBeGreaterThan(0);
  expect(seen.refusedByHeader).toBeGreaterThan(0);
  expect(seen.replaysRefusedByHeader).toBeGreaterThan(0);
  expect(seen.windowSteps).toBeGreaterThan(0);
});
