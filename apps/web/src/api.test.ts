// Les refus de session d'`api.ts` (Rulings R347, R348) : un appel parti avant que l'onglet ouvre
// une session (connexion, inscription, changement de mot de passe), et dont le `401
// unauthenticated` revient après, ne dit rien de la session ouverte depuis. Il ne demande pas la
// connexion : il repart une fois, sous le même compte, si le serveur dit juste avant que la session
// est toujours la sienne. Jamais sous un autre compte, jamais après une déconnexion. Un refus de la
// session en cours, lui, demande la connexion, comme avant.
//
// `fetch` est remplacé par un serveur que le test fait répondre, appel par appel ; `api.ts` est
// rechargé à chaque test (son état, la session de l'onglet, est celui du module).
import type { Me, PageNode, Tree } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { page } from "./test/pages";

type Api = typeof import("./api");

/** Un appel reçu par le serveur du test, auquel il répond quand il veut. */
type Received = {
  method: string;
  path: string;
  body: unknown;
  /** Le corps tel qu'il part, quand ce n'est pas du JSON (un fichier). */
  raw: unknown;
  /** Le type que l'appel déclare pour son corps. */
  type: string | undefined;
  /** Le compte que l'appel nomme (`X-Mementum-Account`, R540) ; `undefined` : aucun. */
  account: string | undefined;
  answered: boolean;
  answer(status: number, body?: unknown): void;
  /** Pas de réponse : le réseau est coupé. */
  fail(): void;
  /**
   * Les en-têtes arrivent (le navigateur pose le cookie, et `fetch` est résolu), puis la connexion
   * se coupe pendant le corps : sa lecture échoue.
   */
  answerLostBody(status: number): void;
};

let api: Api;
let received: Received[];
/** Combien de fois `onSessionLost` a demandé la connexion. */
let asked: number;

const me = (id: string): Me => ({
  user: { id, email: `${id}@exemple.fr`, name: id, instanceAdmin: false },
  spaces: [],
});
const UNAUTHENTICATED = {
  error: "unauthenticated",
  message: "session absente ou échue : se connecter",
};
const INVALID_CREDENTIALS = { error: "invalid_credentials", message: "Mot de passe incorrect." };
const ACCOUNT_CHANGED = {
  error: "account_changed",
  message: "tu t'es connecté sous un autre compte dans un autre onglet ; rien n'a été fait",
};
const PASSWORDS = { current: "ancien-mot-de-passe", new: "nouveau-mot-de-passe" };
/** Ce que répond un changement de mot de passe réussi : les jetons d'API révoqués (R1365). */
const PASSWORD_CHANGED = { tokensRevoked: 2 };
const tree = (spaceId: string): Tree => ({ spaceId, lsn: 1, nodes: [] });

beforeEach(async () => {
  vi.resetModules();
  received = [];
  asked = 0;
  vi.stubGlobal(
    "fetch",
    (path: string, init: RequestInit = {}) =>
      new Promise<Response>((resolve, reject) => {
        const call: Received = {
          method: init.method ?? "GET",
          path,
          body: typeof init.body === "string" ? JSON.parse(init.body) : undefined,
          raw: typeof init.body === "string" ? undefined : init.body,
          type: new Headers(init.headers).get("content-type") ?? undefined,
          account: new Headers(init.headers).get("x-mementum-account") ?? undefined,
          answered: false,
          answer(status, body) {
            call.answered = true;
            resolve(new Response(body === undefined ? null : JSON.stringify(body), { status }));
          },
          fail() {
            call.answered = true;
            reject(new TypeError("Failed to fetch"));
          },
          answerLostBody(status) {
            call.answered = true;
            resolve({
              ok: status >= 200 && status < 300,
              status,
              json: () => Promise.reject(new TypeError("network error")),
            } as unknown as Response);
          },
        };
        received.push(call);
      }),
  );
  api = await import("./api");
  api.onSessionLost(() => {
    asked += 1;
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** Laisse les promesses suivre, le temps de quelques tours. */
async function flush(): Promise<void> {
  for (let i = 0; i < 5; i++) await new Promise((done) => setTimeout(done, 0));
}

/** Le premier appel `method path` qui attend sa réponse ; attendu au plus deux secondes. */
async function waiting(method: string, path: string): Promise<Received> {
  const end = performance.now() + 2_000;
  for (;;) {
    const call = received.find((c) => !c.answered && c.method === method && c.path === path);
    if (call) return call;
    if (performance.now() > end) {
      const seen = received.map((c) => `${c.method} ${c.path}${c.answered ? "" : " (en attente)"}`);
      throw new Error(`toujours pas d'appel ${method} ${path} ; reçus : ${seen.join(", ")}`);
    }
    await new Promise((done) => setTimeout(done, 0));
  }
}

/** Ce que le serveur a reçu, dans l'ordre. */
const log = () => received.map((c) => `${c.method} ${c.path}`);
/** Combien de fois il a reçu `method path`. */
const count = (method: string, path: string) =>
  received.filter((c) => c.method === method && c.path === path).length;

/** L'application chargée sous `id` : `GET /api/v1/me` a répondu. */
async function loadedAs(id: string): Promise<void> {
  const loading = api.getMe();
  (await waiting("GET", "/api/v1/me")).answer(200, me(id));
  await loading;
}

/** L'onglet ouvre une session sous `id`, par la connexion. */
async function logIn(id: string): Promise<void> {
  const login = api.logIn({ email: `${id}@exemple.fr`, password: "mot-de-passe" });
  (await waiting("POST", "/api/v1/auth/login")).answer(200, me(id));
  await login;
}

/** L'onglet change son mot de passe : le serveur le remplace, et rouvre la session. */
async function changePassword(): Promise<void> {
  const change = api.changePassword(PASSWORDS);
  (await waiting("POST", "/api/v1/auth/password")).answer(200, PASSWORD_CHANGED);
  await change;
}

/** L'onglet se déconnecte. */
async function logOut(): Promise<void> {
  const out = api.logOut();
  (await waiting("POST", "/api/v1/auth/logout")).answer(204);
  await out;
}

test("a_401_of_the_current_session_asks_to_log_in_and_is_not_sent_again", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  (await waiting("GET", "/api/v1/pages/p1")).answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ status: 401, code: "unauthenticated" });
  await flush();
  expect(asked).toBe(1);
  expect(log()).toEqual(["GET /api/v1/me", "GET /api/v1/pages/p1"]);
});

test("a_401_sent_before_the_login_and_answered_after_it_is_sent_again_without_asking_to_log_in", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  // La session est perdue ; l'onglet en rouvre une, sous le même compte, pendant que la lecture
  // partie sans elle est en route. Son refus revient après.
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  await flush();
  expect(asked).toBe(0);
  // Le compte de la session est relu, puis la lecture repart, une fois, et aboutit.
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await expect(read).resolves.toEqual(page("p1"));
  expect(asked).toBe(0);
  expect(log()).toEqual([
    "GET /api/v1/me",
    "GET /api/v1/pages/p1",
    "POST /api/v1/auth/login",
    "GET /api/v1/me",
    "GET /api/v1/pages/p1",
  ]);
});

test("a_write_refused_before_the_login_is_sent_again_once_with_the_same_body", async () => {
  await loadedAs("alice");
  const created = api.createPage({ spaceId: "s1", section: "private", title: "Notes" });
  const first = await waiting("POST", "/api/v1/pages");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("POST", "/api/v1/pages");
  expect(again.body).toEqual(first.body);
  const made: PageNode = page("p1", { title: "Notes", section: "private" });
  again.answer(201, made);
  await expect(created).resolves.toEqual(made);
  await flush();
  expect(count("POST", "/api/v1/pages")).toBe(2);
  expect(asked).toBe(0);
});

test("a_call_is_sent_again_once_at_most", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("GET", "/api/v1/pages/p1");
  // Une autre session s'ouvre pendant le rejeu : son refus vise lui aussi une session remplacée,
  // mais l'appel ne repart plus. Il ne demande pas non plus la connexion : il ne dit rien de la
  // session en cours.
  await logIn("alice");
  again.answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ status: 401, code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/pages/p1")).toBe(2);
  expect(asked).toBe(0);
});

test("a_call_sent_again_and_refused_by_the_session_in_course_asks_to_log_in", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  // La session rouverte est perdue à son tour : le rejeu le dit, une fois.
  (await waiting("GET", "/api/v1/pages/p1")).answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ status: 401, code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/pages/p1")).toBe(2);
  expect(asked).toBe(1);
});

test("a_401_answered_after_a_login_under_another_account_is_never_sent_again", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  // C'est Bruno qui se connecte par-dessus l'application d'Alice.
  await logIn("bruno");
  first.answer(401, UNAUTHENTICATED);
  await expect(write).rejects.toMatchObject({ status: 401, code: "unauthenticated" });
  await flush();
  // Rien ne part plus : ni le titre d'Alice sous le compte de Bruno, ni une relecture du compte.
  expect(log()).toEqual(["GET /api/v1/me", "PATCH /api/v1/pages/p1", "POST /api/v1/auth/login"]);
  expect(asked).toBe(0);
});

test("a_call_is_not_sent_again_when_the_session_has_passed_to_another_account", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  // Entre-temps, Bruno s'est connecté dans un autre onglet du navigateur, qui partage le cookie :
  // le compte relu avant le rejeu le dit.
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await expect(write).rejects.toMatchObject({ status: 401, code: "unauthenticated" });
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);

  // Ce que l'onglet a appris vaut pour la suite : un appel parti sous Alice n'est pas rejoué,
  // même si la session redevient la sienne avant que son refus revienne.
  const other = api.getPage("p2");
  const sent = await waiting("GET", "/api/v1/pages/p2");
  await logIn("alice");
  sent.answer(401, UNAUTHENTICATED);
  await expect(other).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/pages/p2")).toBe(1);
});

test("a_call_sent_once_the_session_is_known_to_be_another_account_is_never_sent_again", async () => {
  // Trouvé par `sessionReplaysAtRandom.test.ts` (graine 169).
  await loadedAs("alice");
  const first = api.getPage("p1").catch(() => {});
  const sent = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  sent.answer(401, UNAUTHENTICATED);
  // La relecture d'avant le rejeu apprend que la session est à Bruno (un autre onglet).
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await first;
  // L'application d'Alice, qui ne le sait pas encore, écrit : l'appel part avec le cookie de
  // Bruno, sans qu'on y puisse rien. Puis Bruno se connecte dans l'onglet ; le refus revient.
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const refused = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("bruno");
  refused.answer(401, UNAUTHENTICATED);
  await flush();
  // Le titre d'Alice ne repart pas sous la session de Bruno, et le compte n'est pas relu pour cela.
  expect(count("GET", "/api/v1/me")).toBe(2);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_call_sent_once_the_account_read_again_is_another_is_never_sent_again", async () => {
  await loadedAs("alice");
  // La relecture du compte (retour sur l'onglet) dit Bruno : l'application va repartir de zéro
  // (R115), mais ce qu'elle a envoyé entre-temps n'est pas à Bruno.
  const reading = api.getMe();
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await reading;
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const refused = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("bruno");
  refused.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(2);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_call_that_left_before_a_logout_is_never_sent_again", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  const read = api.getPage("p2");
  const second = await waiting("GET", "/api/v1/pages/p2");
  await logOut();
  // Le refus revenu après la déconnexion : elle l'explique, il ne demande pas la connexion.
  second.answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  // Alice se reconnecte dans l'onglet : ce qui était parti avant la déconnexion ne repart pas.
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(log()).toEqual([
    "GET /api/v1/me",
    "PATCH /api/v1/pages/p1",
    "GET /api/v1/pages/p2",
    "POST /api/v1/auth/logout",
    "POST /api/v1/auth/login",
  ]);
  expect(asked).toBe(0);
});

test("a_call_that_left_during_a_logout_is_never_sent_again", async () => {
  await loadedAs("alice");
  const out = api.logOut();
  const logout = await waiting("POST", "/api/v1/auth/logout");
  const read = api.getPage("p1");
  const sent = await waiting("GET", "/api/v1/pages/p1");
  logout.answer(204);
  await out;
  await logIn("alice");
  sent.answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/pages/p1")).toBe(1);
  expect(count("GET", "/api/v1/me")).toBe(1);
  expect(asked).toBe(0);
});

test("a_401_answered_after_a_password_change_is_sent_again_without_asking_to_log_in", async () => {
  await loadedAs("alice");
  const read = api.getTree("s1");
  const first = await waiting("GET", "/api/v1/spaces/s1/tree");
  // Le changement de mot de passe ferme toutes les sessions du compte, dont celle sous laquelle
  // la lecture est partie, et rouvre celle de l'onglet avec un cookie neuf.
  await changePassword();
  first.answer(401, UNAUTHENTICATED);
  await flush();
  expect(asked).toBe(0);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/spaces/s1/tree")).answer(200, tree("s1"));
  await expect(read).resolves.toEqual(tree("s1"));
  expect(asked).toBe(0);
});

test("a_401_that_comes_back_during_a_password_change_waits_for_it", async () => {
  await loadedAs("alice");
  const read = api.getTree("s1");
  const first = await waiting("GET", "/api/v1/spaces/s1/tree");
  const change = api.changePassword(PASSWORDS);
  const password = await waiting("POST", "/api/v1/auth/password");
  // Le serveur a changé le mot de passe, puis refusé la lecture ; son refus arrive le premier.
  first.answer(401, UNAUTHENTICATED);
  await flush();
  expect(asked).toBe(0);
  password.answer(200, PASSWORD_CHANGED);
  await change;
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/spaces/s1/tree")).answer(200, tree("s1"));
  await expect(read).resolves.toEqual(tree("s1"));
  expect(asked).toBe(0);
});

test("a_password_change_says_its_revoked_tokens_and_survives_a_lost_body", async () => {
  // R1375 : la réponse dit les jetons d'API révoqués ; un corps perdu après les en-têtes (le
  // cookie est posé, le changement fait) ne fait pas échouer le changement.
  await loadedAs("alice");
  const change = api.changePassword(PASSWORDS);
  (await waiting("POST", "/api/v1/auth/password")).answer(200, PASSWORD_CHANGED);
  await expect(change).resolves.toEqual(PASSWORD_CHANGED);
  const lost = api.changePassword(PASSWORDS);
  (await waiting("POST", "/api/v1/auth/password")).answerLostBody(200);
  await expect(lost).resolves.toBeNull();
  // La session reste sûre : un appel suivant part au nom du compte, sans relecture du compte.
  const read = api.getTree("s1");
  const sent = await waiting("GET", "/api/v1/spaces/s1/tree");
  expect(sent.account).toBe("alice");
  sent.answer(200, tree("s1"));
  await expect(read).resolves.toEqual(tree("s1"));
  expect(count("GET", "/api/v1/me")).toBe(1);
  expect(asked).toBe(0);
});

test("token_routes_go_through_send_and_a_lost_creation_body_is_a_lost_answer", async () => {
  // Lot 5, Task C1 : les jetons d'API passent par `send`, qui nomme le compte (R540) ; le corps
  // d'une création perdu après son `201` est une réponse perdue (R757) : le jeton existe peut-être.
  await loadedAs("alice");
  const listed = api.listTokens();
  const list = await waiting("GET", "/api/v1/tokens");
  expect(list.account).toBe("alice");
  list.answer(200, []);
  await expect(listed).resolves.toEqual([]);

  const body = { name: "Claude Code", level: "read", scope: "all", canTrash: false, password: "x" } as const;
  const created = api.createToken(body);
  const create = await waiting("POST", "/api/v1/tokens");
  expect(create.account).toBe("alice");
  expect(create.body).toEqual(body);
  create.answerLostBody(201);
  const lost = await created.catch((e: unknown) => e);
  expect(api.answerLost(lost)).toBe(true);

  const revoked = api.revokeToken("t/1");
  const revoke = await waiting("DELETE", "/api/v1/tokens/t%2F1");
  expect(revoke.account).toBe("alice");
  revoke.answer(204);
  await expect(revoked).resolves.toBeUndefined();

  const calls = api.tokenCalls("t1", { before: 42, outcome: "refused" });
  (await waiting("GET", "/api/v1/tokens/t1/calls?before=42&outcome=refused")).answer(200, {
    calls: [],
    more: false,
    busyCount: 0,
    purged: 0,
  });
  await expect(calls).resolves.toMatchObject({ more: false });
});

test("a_401_that_comes_back_during_a_refused_password_change_asks_to_log_in", async () => {
  await loadedAs("alice");
  const read = api.getTree("s1");
  const first = await waiting("GET", "/api/v1/spaces/s1/tree");
  const change = api.changePassword(PASSWORDS);
  const password = await waiting("POST", "/api/v1/auth/password");
  first.answer(401, UNAUTHENTICATED);
  // Mot de passe actuel faux : rien n'a changé, la session était donc bien perdue.
  password.answer(401, INVALID_CREDENTIALS);
  await expect(change).rejects.toMatchObject({ code: "invalid_credentials" });
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(asked).toBe(1);
  expect(count("GET", "/api/v1/spaces/s1/tree")).toBe(1);
});

test("a_401_that_comes_back_during_a_login_waits_for_it", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  const login = api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" });
  const sent = await waiting("POST", "/api/v1/auth/login");
  first.answer(401, UNAUTHENTICATED);
  await flush();
  expect(asked).toBe(0);
  sent.answer(200, me("alice"));
  await login;
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await expect(read).resolves.toEqual(page("p1"));
  expect(asked).toBe(0);
});

test("a_session_route_is_never_sent_again", async () => {
  await loadedAs("alice");
  // Le changement de mot de passe part sous une session déjà perdue ; l'onglet en rouvre une
  // pendant ce temps. Son refus ne le fait pas repartir : les mots de passe tapés ne sont pas
  // renvoyés d'office sous la session neuve.
  const change = api.changePassword(PASSWORDS);
  const first = await waiting("POST", "/api/v1/auth/password");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  await expect(change).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("POST", "/api/v1/auth/password")).toBe(1);
  expect(count("GET", "/api/v1/me")).toBe(1);
  expect(asked).toBe(0);
});

test("a_session_route_refused_by_the_session_in_course_asks_to_log_in", async () => {
  await loadedAs("alice");
  const change = api.changePassword(PASSWORDS);
  (await waiting("POST", "/api/v1/auth/password")).answer(401, UNAUTHENTICATED);
  await expect(change).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(asked).toBe(1);
});

test("a_call_is_not_sent_again_when_the_account_cannot_be_read_again", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  // Le serveur ne répond plus : l'appel le dit, sans repartir ni demander la connexion.
  (await waiting("GET", "/api/v1/me")).fail();
  await expect(read).rejects.toMatchObject({ status: 0, code: null });
  await flush();
  expect(count("GET", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);
});

test("a_login_under_another_account_during_the_reading_of_the_account_stops_the_replay", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  // Bruno se connecte dans l'onglet pendant la relecture, partie sous la session d'Alice, qui
  // répond donc Alice : le titre d'Alice ne part pas sous la session de Bruno.
  await logIn("bruno");
  reading.answer(200, me("alice"));
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);

  // Cette réponse ne dit pas non plus le compte de la session en cours, toujours Bruno : un appel
  // parti sous Bruno, refusé après une autre connexion de Bruno, repart.
  const read = api.getPage("p2");
  const sent = await waiting("GET", "/api/v1/pages/p2");
  await logIn("bruno");
  sent.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  (await waiting("GET", "/api/v1/pages/p2")).answer(200, page("p2"));
  await expect(read).resolves.toEqual(page("p2"));
});

test("a_reply_of_a_replaced_session_does_not_say_the_account", async () => {
  await loadedAs("alice");
  // Une relecture du compte part sous la session d'Alice ; Bruno se connecte dans l'onglet avant
  // sa réponse, qui dit donc Alice.
  const reading = api.getMe();
  const sent = await waiting("GET", "/api/v1/me");
  await logIn("bruno");
  sent.answer(200, me("alice"));
  await reading;
  // Le compte de la session reste Bruno : un appel parti sous lui, refusé après une autre
  // connexion de Bruno, repart.
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("bruno");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await expect(read).resolves.toEqual(page("p1"));
  expect(asked).toBe(0);
});

test("the_reading_of_the_account_before_a_replay_is_never_sent_again", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1").catch((e: unknown) => e);
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  // Une autre connexion pendant la relecture : son refus vise une session remplacée, mais elle ne
  // repart pas (ni relecture de la relecture), et l'appel non plus.
  await logIn("alice");
  reading.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(2);
  expect(await read).toMatchObject({ code: "unauthenticated" });
  expect(count("GET", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);
});

test("a_call_that_left_under_no_known_account_is_never_sent_again", async () => {
  // Aucun compte encore dit par le serveur (l'application n'est pas chargée).
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await changePassword();
  first.answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(log()).toEqual(["GET /api/v1/pages/p1", "POST /api/v1/auth/password"]);
  expect(asked).toBe(0);
});

test("a_failed_logout_still_keeps_what_left_before_it_from_asking_to_log_in", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  const out = api.logOut();
  // Pas de réponse : la session a pu être fermée quand même.
  (await waiting("POST", "/api/v1/auth/logout")).fail();
  await expect(out).rejects.toMatchObject({ status: 0 });
  first.answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);
  // Un appel parti ensuite, et refusé, dit la session perdue.
  const after = api.getPage("p2");
  (await waiting("GET", "/api/v1/pages/p2")).answer(401, UNAUTHENTICATED);
  await expect(after).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(asked).toBe(1);
});

test("a_call_is_not_sent_again_when_the_session_is_lost_again_before_it", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  // La session rouverte est déjà perdue quand le compte est relu : la connexion est demandée, une
  // fois, et l'appel ne repart pas.
  (await waiting("GET", "/api/v1/me")).answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(1);
});

// Tour de correction 1 (R461 à R464, R480).

test("an_account_read_answered_during_a_failed_logout_does_not_rearm_the_replay", async () => {
  // Sonde I1 de la relecture. Le menu de l'espace relit le compte ; on clique « Se déconnecter »
  // aussitôt ; la relecture répond Alice pendant la déconnexion.
  await loadedAs("alice");
  const reading = api.getMe();
  const read = await waiting("GET", "/api/v1/me");
  const out = api.logOut();
  const logout = await waiting("POST", "/api/v1/auth/logout");
  read.answer(200, me("alice"));
  await reading;
  // L'application, toujours montée, écrit pendant la déconnexion.
  const write = api.updatePage("p1", { title: "Pendant la déconnexion" });
  write.catch(() => {});
  const sent = await waiting("PATCH", "/api/v1/pages/p1");
  // La réponse de la déconnexion se perd : l'application reste montée. Alice se reconnecte, et le
  // refus de l'écriture revient : partie pendant une déconnexion, elle ne repart jamais.
  logout.fail();
  await expect(out).rejects.toMatchObject({ status: 0 });
  await logIn("alice");
  sent.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(2);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);
});

test("an_account_read_answered_during_a_logout_does_not_rearm_the_replay_under_another_tab_session", async () => {
  await loadedAs("alice");
  const reading = api.getMe();
  const read = await waiting("GET", "/api/v1/me");
  const out = api.logOut();
  const logout = await waiting("POST", "/api/v1/auth/logout");
  read.answer(200, me("alice"));
  await reading;
  const write = api.updatePage("p1", { title: "Pendant la déconnexion" });
  write.catch(() => {});
  const sent = await waiting("PATCH", "/api/v1/pages/p1");
  logout.answer(204);
  await out;
  // Avant le rechargement de l'onglet, un autre onglet a reconnecté Alice ; le refus revient.
  sent.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(2);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);
});

test("a_call_sent_during_a_logout_crossed_by_a_login_is_never_sent_again", async () => {
  await loadedAs("alice");
  const out = api.logOut();
  const logout = await waiting("POST", "/api/v1/auth/logout");
  // Une connexion de l'onglet aboutit pendant la déconnexion (deux formulaires) ; l'application
  // écrit, la déconnexion toujours en route.
  await logIn("alice");
  const write = api.updatePage("p1", { title: "Pendant la déconnexion" });
  write.catch(() => {});
  const sent = await waiting("PATCH", "/api/v1/pages/p1");
  logout.answer(204);
  await out;
  await logIn("alice");
  sent.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(1);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("after_a_failed_logout_the_application_is_still_the_one_it_was", async () => {
  // La déconnexion échoue : l'application reste celle d'Alice. Une relecture du compte qui trouve
  // alors Bruno (un autre onglet) ne la charge pas sous Bruno : ce qu'elle écrit n'est pas à lui.
  await loadedAs("alice");
  const out = api.logOut();
  (await waiting("POST", "/api/v1/auth/logout")).fail();
  await expect(out).rejects.toMatchObject({ status: 0 });
  const reading = api.getMe();
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await reading;
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const sent = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("bruno");
  sent.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(2);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_failed_logout_lets_the_replay_work_again_once_the_account_is_read", async () => {
  // Garde : la déconnexion finie, même échouée, les appels sont de nouveau datés.
  await loadedAs("alice");
  const out = api.logOut();
  (await waiting("POST", "/api/v1/auth/logout")).fail();
  await expect(out).rejects.toMatchObject({ status: 0 });
  await loadedAs("alice");
  const read = api.getPage("p1");
  const sent = await waiting("GET", "/api/v1/pages/p1");
  await changePassword();
  sent.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await expect(read).resolves.toEqual(page("p1"));
});

test("a_replay_gives_up_when_a_login_of_the_tab_crosses_the_second_reading_too", async () => {
  // Garde : une session de l'onglet croise la relecture, puis une autre croise la seconde ; on ne
  // relit pas sans fin, l'appel ne repart pas.
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" });
  const opening = await waiting("POST", "/api/v1/auth/login");
  reading.answer(200, me("alice"));
  opening.answer(200, me("alice"));
  await login;
  const second = await waiting("GET", "/api/v1/me");
  const again = api.logIn({ email: "bruno@exemple.fr", password: "mot-de-passe" });
  const reopening = await waiting("POST", "/api/v1/auth/login");
  second.answer(200, me("alice"));
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  reopening.answer(200, me("bruno"));
  await again;
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_401_that_comes_back_after_a_login_without_answer_does_not_ask_to_log_in", async () => {
  // Garde : une connexion sans réponse a pu fermer la session sous laquelle l'appel est parti
  // (R73) ; son refus ne dit rien de sûr de la session en cours, et ne demande pas la connexion.
  await loadedAs("alice");
  const read = api.getPage("p1");
  const sent = await waiting("GET", "/api/v1/pages/p1");
  const login = api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" });
  (await waiting("POST", "/api/v1/auth/login")).fail();
  await expect(login).rejects.toMatchObject({ status: 0 });
  sent.answer(401, UNAUTHENTICATED);
  await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(asked).toBe(0);
  expect(count("GET", "/api/v1/pages/p1")).toBe(1);
});

test("a_replay_waits_for_a_login_of_the_tab_in_course_and_stops_if_it_is_another_account", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  // Bruno se connecte dans l'onglet pendant la relecture ; le serveur a ouvert sa session (le
  // navigateur porte peut-être déjà son cookie), la page ne le sait pas encore. La relecture,
  // partie avant, répond Alice : le titre ne part pas avant de savoir.
  const login = api.logIn({ email: "bruno@exemple.fr", password: "mot-de-passe" });
  const opening = await waiting("POST", "/api/v1/auth/login");
  reading.answer(200, me("alice"));
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  // C'est Bruno : le titre d'Alice ne part pas.
  opening.answer(200, me("bruno"));
  await login;
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  expect(asked).toBe(0);
});

test("a_replay_waits_for_a_login_of_the_tab_in_course_and_goes_if_it_is_the_same_account", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" });
  const opening = await waiting("POST", "/api/v1/auth/login");
  reading.answer(200, me("alice"));
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  // C'est encore Alice : le compte est relu sous la session qu'elle vient d'ouvrir (la première
  // relecture est partie avant elle), puis le titre part.
  opening.answer(200, me("alice"));
  await login;
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("PATCH", "/api/v1/pages/p1")).answer(200, page("p1", { title: "Titre d'Alice" }));
  await expect(write).resolves.toMatchObject({ title: "Titre d'Alice" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(2);
  expect(count("GET", "/api/v1/me")).toBe(3);
});

test("a_replay_reads_the_account_again_after_a_login_of_the_tab_that_crossed_the_reading", async () => {
  // Si la seconde relecture, sous la session neuve, trouve Bruno (un autre onglet), rien ne part.
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" });
  const opening = await waiting("POST", "/api/v1/auth/login");
  reading.answer(200, me("alice"));
  opening.answer(200, me("alice"));
  await login;
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_stale_refusal_waits_for_a_login_of_the_tab_in_course_before_reading_the_account", async () => {
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  const login = api.logIn({ email: "bruno@exemple.fr", password: "mot-de-passe" });
  const opening = await waiting("POST", "/api/v1/auth/login");
  first.answer(401, UNAUTHENTICATED);
  await flush();
  // Rien ne part tant que la connexion en route n'a pas dit son compte.
  expect(count("GET", "/api/v1/me")).toBe(1);
  opening.answer(200, me("bruno"));
  await login;
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(1);
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_login_whose_answer_is_lost_keeps_what_left_before_it_from_being_sent_again", async () => {
  // Trouvé par l'essai au hasard rendu fidèle (réponses perdues) : le cookie d'une connexion de
  // Bruno a pu être posé alors que sa réponse s'est perdue ; la page ne sait pas que la session a
  // changé, et la relecture du compte, partie avant, dit Alice.
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "bruno@exemple.fr", password: "mot-de-passe" });
  (await waiting("POST", "/api/v1/auth/login")).fail();
  await expect(login).rejects.toMatchObject({ status: 0 });
  reading.answer(200, me("alice"));
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(asked).toBe(0);
  // Le compte relu par l'application rend le rejeu possible de nouveau.
  await loadedAs("alice");
  const read = api.getPage("p2");
  const sent = await waiting("GET", "/api/v1/pages/p2");
  await changePassword();
  sent.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p2")).answer(200, page("p2"));
  await expect(read).resolves.toEqual(page("p2"));
});

test("a_login_whose_body_is_lost_after_its_headers_keeps_the_replay_from_going", async () => {
  // Re-relecture, N1 : la forme qu'a, dans un navigateur, « une réponse perdue après son cookie ».
  // Les en-têtes de la connexion de Bruno sont arrivés (cookie posé, `fetch` résolu) ; la connexion
  // se coupe pendant le corps, dont la lecture lève une `TypeError`, pas une `ApiError` de statut 0.
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "bruno@exemple.fr", password: "mot-de-passe" });
  login.catch(() => {});
  (await waiting("POST", "/api/v1/auth/login")).answerLostBody(200);
  await expect(login).rejects.toBeInstanceOf(TypeError);
  // La relecture, partie avec le cookie d'Alice avant la connexion, dit Alice ; le titre ne repart
  // pas (il porterait le cookie de Bruno).
  reading.answer(200, me("alice"));
  await flush();
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
  expect(count("GET", "/api/v1/me")).toBe(2);
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
});

test("another_account_is_not_told_while_a_login_of_the_tab_is_in_course", async () => {
  // Re-relecture, N2 : la relecture, traitée quand un autre onglet tenait la session sous Bruno,
  // répond Bruno ; mais l'onglet se reconnecte sous Alice (sa connexion ferme la session de Bruno,
  // R73). Annoncer tout de suite ferait recharger l'onglet en pleine reconnexion sous son compte.
  const told: string[] = [];
  api.onOtherAccount((other) => told.push(other.user.id));
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" });
  const sent = await waiting("POST", "/api/v1/auth/login");
  reading.answer(200, me("bruno"));
  await flush();
  expect(told).toEqual([]);
  sent.answer(200, me("alice"));
  await login;
  await flush();
  // La session est revenue à Alice : rien à dire.
  expect(told).toEqual([]);
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("another_account_found_during_a_password_change_of_the_tab_is_told_once_it_settles", async () => {
  // Témoin du précédent : la session de l'onglet qui s'est changée reste à Bruno (son mot de passe
  // a changé sous la session de Bruno, que portait le cookie) ; l'annonce part à son issue.
  const told: string[] = [];
  api.onOtherAccount((other) => told.push(other.user.id));
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const change = api.changePassword(PASSWORDS);
  const sent = await waiting("POST", "/api/v1/auth/password");
  reading.answer(200, me("bruno"));
  await flush();
  expect(told).toEqual([]);
  sent.answer(200, PASSWORD_CHANGED);
  await change;
  await flush();
  expect(told).toEqual(["bruno"]);
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("an_old_refusal_at_the_bound_does_not_ask_to_log_in", async () => {
  // Re-relecture, N3 (mutation C14b) : à la borne, un refus d'une génération ancienne ne repart pas
  // et ne demande pas la connexion ; seul celui de la génération en cours la demande.
  await loadedAs("alice");
  const read = api.getPage("p1");
  read.catch(() => {});
  const first = await waiting("GET", "/api/v1/pages/p1");
  await changePassword();
  // Un second changement, resté sans réponse, est en route quand le refus revient.
  void api.changePassword(PASSWORDS).catch(() => {});
  await waiting("POST", "/api/v1/auth/password");
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    first.answer(401, UNAUTHENTICATED);
    await vi.advanceTimersByTimeAsync(10_100);
    await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
    expect(asked).toBe(0);
    expect(count("GET", "/api/v1/pages/p1")).toBe(1);
  } finally {
    vi.useRealTimers();
  }
});

test("a_reading_is_not_shared_across_a_session_change_of_the_tab", async () => {
  // Re-relecture, N4 (mutation C17) : une relecture partie avant un changement de session de
  // l'onglet ne sert pas à un refus jugé après lui ; la partager élargirait la fenêtre de R460 de
  // toute la durée du changement (R463).
  await loadedAs("alice");
  const y = api.getPage("p1");
  const x = api.getPage("p2");
  y.catch(() => {});
  x.catch(() => {});
  const sentY = await waiting("GET", "/api/v1/pages/p1");
  const sentX = await waiting("GET", "/api/v1/pages/p2");
  await changePassword();
  sentY.answer(401, UNAUTHENTICATED);
  await waiting("GET", "/api/v1/me");
  // Le mot de passe change encore pendant cette relecture.
  await changePassword();
  sentX.answer(401, UNAUTHENTICATED);
  await flush();
  const readings = received.filter((c) => !c.answered && c.method === "GET" && c.path === "/api/v1/me");
  expect(readings.length).toBe(2);
});

test("a_login_refused_for_its_password_changes_nothing", async () => {
  // Témoin du précédent : un mot de passe faux est une réponse ; la session n'a pas changé.
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  const login = api.logIn({ email: "alice@exemple.fr", password: "faux" });
  (await waiting("POST", "/api/v1/auth/login")).answer(401, INVALID_CREDENTIALS);
  await expect(login).rejects.toMatchObject({ code: "invalid_credentials" });
  reading.answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await expect(read).resolves.toEqual(page("p1"));
});

test("the_replay_sends_the_body_as_it_was_first_sent", async () => {
  // Sonde du mineur 2 : un objet changé par l'appelant après l'envoi ne change pas le rejeu.
  await loadedAs("alice");
  const body = { title: "Avant" };
  const write = api.updatePage("p1", body);
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  body.title = "Changé par l'appelant après l'envoi";
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("PATCH", "/api/v1/pages/p1");
  expect(again.body).toEqual({ title: "Avant" });
});

test("refusals_of_one_session_share_one_reading_of_the_account", async () => {
  // Sonde du mineur 3 : cinq appels en route pendant un changement de mot de passe.
  await loadedAs("alice");
  const reads = [1, 2, 3, 4, 5].map((n) => api.getPage(`p${n}`));
  const sent = await Promise.all([1, 2, 3, 4, 5].map((n) => waiting("GET", `/api/v1/pages/p${n}`)));
  await changePassword();
  for (const call of sent) call.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(2);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  for (const n of [1, 2, 3, 4, 5]) (await waiting("GET", `/api/v1/pages/p${n}`)).answer(200, page(`p${n}`));
  expect(await Promise.all(reads)).toEqual([1, 2, 3, 4, 5].map((n) => page(`p${n}`)));
  expect(count("GET", "/api/v1/me")).toBe(2);
});

test("a_reading_of_the_account_is_not_shared_once_answered", async () => {
  // Témoin du précédent : un refus revenu après la réponse de la relecture en refait une.
  await loadedAs("alice");
  const one = api.getPage("p1");
  const two = api.getPage("p2");
  const [first, second] = [await waiting("GET", "/api/v1/pages/p1"), await waiting("GET", "/api/v1/pages/p2")];
  await changePassword();
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await one;
  second.answer(401, UNAUTHENTICATED);
  await flush();
  expect(count("GET", "/api/v1/me")).toBe(3);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p2")).answer(200, page("p2"));
  await expect(two).resolves.toEqual(page("p2"));
});

test("a_session_change_that_never_answers_holds_a_refusal_ten_seconds_at_most", async () => {
  // Sonde du mineur 4 : un changement de mot de passe sans réponse retenait tout refus de la
  // session en cours, sans fin.
  await loadedAs("alice");
  const read = api.getPage("p1");
  let settled = false;
  read.then(
    () => (settled = true),
    () => (settled = true),
  );
  const first = await waiting("GET", "/api/v1/pages/p1");
  void api.changePassword(PASSWORDS).catch(() => {});
  await waiting("POST", "/api/v1/auth/password");
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    first.answer(401, UNAUTHENTICATED);
    await vi.advanceTimersByTimeAsync(9_900);
    expect(settled).toBe(false);
    expect(asked).toBe(0);
    // Au-delà de la borne, la session est tenue pour perdue : la connexion est demandée.
    await vi.advanceTimersByTimeAsync(200);
    expect(settled).toBe(true);
    expect(asked).toBe(1);
    await expect(read).rejects.toMatchObject({ code: "unauthenticated" });
  } finally {
    vi.useRealTimers();
  }
});

test("a_logout_refused_by_the_server_settles_at_once", async () => {
  // Mineur 5 : la déconnexion est une route de session ; refusée (le serveur ne le fait pas
  // aujourd'hui), elle n'attend pas sa propre issue.
  await loadedAs("alice");
  let settled = false;
  const out = api.logOut();
  out.then(
    () => (settled = true),
    () => (settled = true),
  );
  (await waiting("POST", "/api/v1/auth/logout")).answer(401, UNAUTHENTICATED);
  await flush();
  expect(settled).toBe(true);
  await expect(out).rejects.toMatchObject({ code: "unauthenticated" });
});

test("a_reading_of_the_account_that_finds_another_account_tells_the_application", async () => {
  // Mineur 8 : la relecture d'avant un rejeu apprend qu'un autre onglet s'est connecté sous
  // Bruno ; l'application en est prévenue (elle repart de zéro, R115), sans attendre de relire.
  const told: string[] = [];
  api.onOtherAccount((other) => told.push(other.user.id));
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await expect(write).rejects.toMatchObject({ code: "unauthenticated" });
  expect(told).toEqual(["bruno"]);
});

test("a_reading_of_the_account_that_finds_the_same_account_tells_nothing", async () => {
  const told: string[] = [];
  api.onOtherAccount((other) => told.push(other.user.id));
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/pages/p1")).answer(200, page("p1"));
  await read;
  expect(told).toEqual([]);
});

// Limite dite (R348, coût) : un rejeu peut passer après un geste plus récent sur le même réglage, et
// l'écraser. Alice donne l'accès complet à Bruno ; l'appel reste en route pendant la reconnexion ;
// reconnectée, elle le baisse à « Peut lire » (abouti) ; le refus du premier revient, il repart.
// Le test échoue, comme attendu, par cette perte et elle seule ; le jour où elle est levée (un rejeu
// qui cède à un appel plus récent au même réglage), il rougit : le sortir alors de `test.fails`.
const OVERWRITTEN = ["full", "read", "full"];
test.fails("a_replay_does_not_overwrite_a_newer_gesture_on_the_same_setting [limite : R348]", async () => {
  await loadedAs("alice");
  const grant = api.setUserLevel("p1", "bruno", "full");
  grant.catch(() => {});
  const first = await waiting("PUT", "/api/v1/pages/p1/access/users/bruno");
  await logIn("alice");
  const lower = api.setUserLevel("p1", "bruno", "read");
  await flush();
  received.filter((c) => c.method === "PUT")[1].answer(200, {});
  await lower;
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  await flush();
  const levels = received
    .filter((c) => c.method === "PUT")
    .map((c) => (c.body as { level: string }).level);
  // Seule la perte dite compte pour l'échec attendu ; toute autre issue fait rougir le test.
  if (JSON.stringify(levels) === JSON.stringify(OVERWRITTEN)) {
    throw new Error(`perte dite : le rejeu écrase le geste plus récent (${levels.join(", ")})`);
  }
});

// Le compte attendu (Rulings R540, R543, R544) : chaque appel nomme le compte de l'application, et
// le serveur refuse par `409 account_changed`, sans rien faire, celui qui part sous la session d'un
// autre compte (un autre onglet s'y est connecté). Le faux serveur répond ce que le vrai répondrait.

test("every_call_names_the_account_of_the_application_but_the_reading_of_the_account_and_the_session_routes", async () => {
  // Avant que l'application ait un compte : rien à nommer.
  const info = api.getInfo();
  (await waiting("GET", "/api/v1/info")).answer(200, { version: "1", schemaVersion: 1 });
  await info;
  expect(api.expectedAccount()).toBeNull();
  await loadedAs("alice");
  void api.getPage("p1").catch(() => {});
  void api.updatePage("p1", { title: "Notes" }).catch(() => {});
  void api.trashPage("p1").catch(() => {});
  void api.getVersionState("p1", "v1").catch(() => {});
  void api.lookupInvitation("jeton").catch(() => {});
  void api.changePassword(PASSWORDS).catch(() => {});
  void api.getMe().catch(() => {});
  void api.logIn({ email: "alice@exemple.fr", password: "mot-de-passe" }).catch(() => {});
  void api
    .signUp({ email: "chloe@exemple.fr", name: "Chloé", password: "mot-de-passe" })
    .catch(() => {});
  void api.logOut().catch(() => {});
  await flush();
  expect(received.map((c) => [`${c.method} ${c.path}`, c.account])).toEqual([
    ["GET /api/v1/info", undefined],
    ["GET /api/v1/me", undefined],
    ["GET /api/v1/pages/p1", "alice"],
    ["PATCH /api/v1/pages/p1", "alice"],
    ["POST /api/v1/pages/p1/trash", "alice"],
    ["GET /api/v1/pages/p1/versions/v1/state", "alice"],
    ["POST /api/v1/invitations/lookup", "alice"],
    ["POST /api/v1/auth/password", "alice"],
    ["GET /api/v1/me", undefined],
    ["POST /api/v1/auth/login", undefined],
    ["POST /api/v1/auth/signup", undefined],
    ["POST /api/v1/auth/logout", undefined],
  ]);
  expect(api.expectedAccount()).toBe("alice");
});

test("a_call_still_names_the_application_once_the_session_is_known_to_be_another_account", async () => {
  await loadedAs("alice");
  // Une relecture du compte trouve la session à Bruno (un autre onglet) : `acting` ne vaut plus
  // rien, mais l'appel nomme toujours Alice, et le serveur le refusera sous la session de Bruno.
  const read = api.getMe();
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await read;
  void api.updatePage("p1", { title: "Notes" }).catch(() => {});
  expect((await waiting("PATCH", "/api/v1/pages/p1")).account).toBe("alice");
  // Pendant une déconnexion aussi.
  void api.logOut().catch(() => {});
  void api.trashPage("p1").catch(() => {});
  expect((await waiting("POST", "/api/v1/pages/p1/trash")).account).toBe("alice");
});

test("a_replay_names_the_account_of_its_first_send", async () => {
  await loadedAs("alice");
  const read = api.getPage("p1");
  const first = await waiting("GET", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("GET", "/api/v1/pages/p1");
  expect(first.account).toBe("alice");
  expect(again.account).toBe("alice");
  again.answer(200, page("p1"));
  await expect(read).resolves.toEqual(page("p1"));
});

test("a_replay_that_meets_the_session_of_another_tab_is_refused_and_tells_the_application", async () => {
  // La fenêtre de R460 : la relecture d'avant le rejeu dit Alice, puis un autre onglet se connecte
  // sous Bruno avant que le rejeu parte. Le rejeu nomme Alice : le serveur le refuse, sans rien
  // faire, et l'application l'apprend.
  await loadedAs("alice");
  const others: string[] = [];
  api.onOtherAccount((found) => others.push(found.user.id));
  const trashed = api.trashPage("p1");
  const first = await waiting("POST", "/api/v1/pages/p1/trash");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("POST", "/api/v1/pages/p1/trash");
  expect(again.account).toBe("alice");
  again.answer(409, ACCOUNT_CHANGED);
  await expect(trashed).rejects.toMatchObject({ status: 409, code: "account_changed" });
  // Le compte est relu : c'est Bruno.
  const reading = await waiting("GET", "/api/v1/me");
  expect(reading.account).toBeUndefined();
  reading.answer(200, me("bruno"));
  await flush();
  expect(others).toEqual(["bruno"]);
  expect(count("POST", "/api/v1/pages/p1/trash")).toBe(2);
  expect(asked).toBe(0);
});

test("a_call_refused_for_another_account_is_never_sent_again_and_tells_the_application", async () => {
  await loadedAs("alice");
  const others: string[] = [];
  api.onOtherAccount((found) => others.push(found.user.id));
  const written = api.updatePage("p1", { title: "Notes" });
  (await waiting("PATCH", "/api/v1/pages/p1")).answer(409, ACCOUNT_CHANGED);
  const failure = await written.catch((e: unknown) => e);
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  expect(failure).toMatchObject({ status: 409, code: "account_changed" });
  expect(api.isSessionLost(failure)).toBe(false);
  await flush();
  expect(others).toEqual(["bruno"]);
  expect(asked).toBe(0);
  expect(log()).toEqual(["GET /api/v1/me", "PATCH /api/v1/pages/p1", "GET /api/v1/me"]);
});

test("a_call_refused_for_another_account_whose_session_came_back_tells_nothing", async () => {
  // Le temps que le refus revienne, la session est revenue au compte de l'application : rien à
  // dire, et l'appel n'est pas rejoué (il n'a rien fait ; la personne le refera).
  await loadedAs("alice");
  const others: string[] = [];
  api.onOtherAccount((found) => others.push(found.user.id));
  const written = api.updatePage("p1", { title: "Notes" });
  (await waiting("PATCH", "/api/v1/pages/p1")).answer(409, ACCOUNT_CHANGED);
  await expect(written).rejects.toMatchObject({ status: 409, code: "account_changed" });
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  await flush();
  expect(others).toEqual([]);
  expect(asked).toBe(0);
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_call_refused_for_another_account_whose_session_is_gone_asks_to_log_in", async () => {
  // L'autre compte s'est déconnecté depuis : la relecture du compte est refusée sans session.
  await loadedAs("alice");
  const written = api.updatePage("p1", { title: "Notes" });
  (await waiting("PATCH", "/api/v1/pages/p1")).answer(409, ACCOUNT_CHANGED);
  await expect(written).rejects.toMatchObject({ status: 409, code: "account_changed" });
  (await waiting("GET", "/api/v1/me")).answer(401, UNAUTHENTICATED);
  await flush();
  expect(asked).toBe(1);
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_password_change_refused_for_another_account_settles_at_once", async () => {
  // Vu par l'essai au hasard (graine 14934 à 30 000 graines, la relecture attendue par le refus) :
  // la relecture du compte faite après un `409 account_changed` partait de l'intérieur du
  // changement de mot de passe, une session de l'onglet en route ; refusée sans session, elle
  // attendait l'issue de ce changement, qui l'attendait elle : tout refus restait bloqué dix
  // secondes. La relecture ne retient pas l'appel refusé.
  await loadedAs("alice");
  const change = api.changePassword(PASSWORDS);
  (await waiting("POST", "/api/v1/auth/password")).answer(409, ACCOUNT_CHANGED);
  await expect(change).rejects.toMatchObject({ status: 409, code: "account_changed" });
  (await waiting("GET", "/api/v1/me")).answer(401, UNAUTHENTICATED);
  await flush();
  // L'autre compte s'est déconnecté : la session est perdue, et la connexion est demandée tout de
  // suite.
  expect(asked).toBe(1);
});

test("a_call_refused_for_another_account_reads_the_account_afresh", async () => {
  // Relecture de la relecture (point 2) : une relecture d'avant un rejeu part avec le cookie
  // d'Alice ; Bruno se connecte ailleurs, ce qui ferme la session d'Alice (R73) ; un appel de
  // l'onglet reçoit `409`. Partager la relecture en route (R463), partie avant le changement de
  // cookie, ne dit rien de lui : refusée sans session, elle demandait la connexion au nom d'Alice,
  // et l'autre compte n'était jamais dit. Le `409` fait donc relire le compte à neuf.
  const others: string[] = [];
  api.onOtherAccount((found) => others.push(found.user.id));
  await loadedAs("alice");
  const write = api.updatePage("p1", { title: "Titre d'Alice" });
  write.catch(() => {});
  const first = await waiting("PATCH", "/api/v1/pages/p1");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  const reading = await waiting("GET", "/api/v1/me");
  // Bruno se connecte ailleurs ; un appel de l'onglet part avec son cookie, et nomme Alice.
  const read = api.getPage("p2");
  read.catch(() => {});
  (await waiting("GET", "/api/v1/pages/p2")).answer(409, ACCOUNT_CHANGED);
  await flush();
  // Deux relectures en route : celle d'avant le rejeu, et une neuve, partie après le `409`.
  expect(received.filter((c) => !c.answered && c.path === "/api/v1/me")).toHaveLength(2);
  reading.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("bruno"));
  await flush();
  expect(others).toEqual(["bruno"]);
  expect(count("PATCH", "/api/v1/pages/p1")).toBe(1);
});

test("a_duplication_is_posted_without_a_body_and_its_refusal_is_never_sent_again", async () => {
  // Dupliquer une page (R252) : `POST …/duplicate`, sans corps, au nom du compte de l'application ;
  // le serveur rend la page racine de la copie.
  await loadedAs("alice");
  const copy = page("copie", { title: "Plan (copie)" });
  const asked1 = api.duplicatePage("p1");
  const first = await waiting("POST", "/api/v1/pages/p1/duplicate");
  expect(first.body).toBeUndefined();
  expect(first.account).toBe("alice");
  first.answer(201, copy);
  await expect(asked1).resolves.toEqual(copy);
  // Le compte duplique déjà, ou a dépensé son budget : `429 duplication_busy` (R605, R606). Ni
  // `api.ts` ni personne ne le rejoue (R612) : le refus est rendu tel quel, une fois, et ne dit
  // rien de la session.
  const busy = {
    error: "duplication_busy",
    message: "tu as déjà une duplication en cours : attends qu'elle finisse avant d'en demander une autre ; rien n'a été copié",
  };
  const asked2 = api.duplicatePage("p1");
  (await waiting("POST", "/api/v1/pages/p1/duplicate")).answer(429, busy);
  await expect(asked2).rejects.toMatchObject({ status: 429, code: "duplication_busy", message: busy.message });
  await flush();
  expect(count("POST", "/api/v1/pages/p1/duplicate")).toBe(2);
  expect(asked).toBe(0);
});

// Fichiers (Task C8) : un envoi est un appel comme un autre. Il part par `send`, nomme le compte de
// l'application (R540), et son refus de session tardif repart une fois, avec le même fichier (R666).

const FILE_INFO = {
  id: "f1",
  url: "/api/v1/pages/p1/files/f1",
  name: "été 2026.png",
  size: 3,
  mime: "image/png",
};

test("an_upload_sends_the_file_as_it_is_with_its_name_in_the_address_and_names_the_account", async () => {
  await loadedAs("alice");
  const file = new File([new Uint8Array([1, 2, 3])], "été 2026 & co?.png", { type: "text/html" });
  const sent = api.uploadFile("p1", file);
  const path = "/api/v1/pages/p1/files?name=%C3%A9t%C3%A9%202026%20%26%20co%3F.png";
  const call = await waiting("POST", path);
  // Le fichier lui-même, pas une copie ni du JSON ; le type déclaré n'est jamais celui du fichier.
  expect(call.raw).toBe(file);
  expect(call.type).toBe("application/octet-stream");
  expect(call.account).toBe("alice");
  call.answer(201, FILE_INFO);
  await expect(sent).resolves.toEqual(FILE_INFO);
});

test("an_upload_refused_before_a_login_is_sent_again_once_with_the_same_file", async () => {
  await loadedAs("alice");
  const file = new File(["abc"], "a.png");
  const sent = api.uploadFile("p1", file);
  const path = "/api/v1/pages/p1/files?name=a.png";
  const first = await waiting("POST", path);
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("POST", path);
  expect(again.raw).toBe(file);
  expect(again.account).toBe("alice");
  again.answer(201, FILE_INFO);
  await expect(sent).resolves.toEqual(FILE_INFO);
  expect(count("POST", path)).toBe(2);
  expect(asked).toBe(0);
});

test("a_cancelled_upload_rejects_with_the_cancellation_and_a_cut_one_with_an_unreachable_server", async () => {
  await loadedAs("alice");
  const controller = new AbortController();
  const cancelled = api.uploadFile("p1", new File(["abc"], "a.png"), controller.signal);
  const first = await waiting("POST", "/api/v1/pages/p1/files?name=a.png");
  controller.abort();
  first.fail();
  await expect(cancelled).rejects.toBeInstanceOf(TypeError);
  // Coupé sans annulation : pas de réponse du serveur (statut 0), ce que l'appelant relit.
  const cut = api.uploadFile("p1", new File(["abc"], "b.png"));
  (await waiting("POST", "/api/v1/pages/p1/files?name=b.png")).fail();
  await expect(cut).rejects.toMatchObject({ status: 0, code: null });
  expect(asked).toBe(0);
});

test("the_limits_of_files_are_read_from_their_route", async () => {
  await loadedAs("alice");
  const limits = api.getFileLimits();
  (await waiting("GET", "/api/v1/files/limits")).answer(200, { maxFileBytes: 1048576 });
  await expect(limits).resolves.toEqual({ maxFileBytes: 1048576 });
});

test("a_copy_of_a_file_names_its_source_in_the_body_and_the_account_in_the_header", async () => {
  await loadedAs("alice");
  const copied = api.copyFile("p2", { pageId: "p1", fileId: "f1" });
  const call = await waiting("POST", "/api/v1/pages/p2/files/copies");
  expect(call.body).toEqual({ pageId: "p1", fileId: "f1" });
  expect(call.type).toBe("application/json");
  expect(call.account).toBe("alice");
  call.answer(201, { ...FILE_INFO, id: "f2", url: "/api/v1/pages/p2/files/f2" });
  await expect(copied).resolves.toMatchObject({ url: "/api/v1/pages/p2/files/f2" });
  // Un refus est rendu tel que le serveur le dit.
  const refused = api.copyFile("p2", { pageId: "p9", fileId: "f1" });
  (await waiting("POST", "/api/v1/pages/p2/files/copies")).answer(422, {
    error: "invalid_source",
    message: "fichier d'origine introuvable",
  });
  await expect(refused).rejects.toMatchObject({ status: 422, code: "invalid_source" });
  expect(asked).toBe(0);
});

// Une réponse perdue après que le serveur a peut-être écrit (relecture finale de la partie B, I1, R757).

test("a_lost_answer_to_a_write_says_it_may_have_been_done_and_to_a_read_that_the_server_is_silent", async () => {
  await loadedAs("alice");
  const MAYBE =
    "La réponse du serveur n'est pas arrivée : ce que tu as demandé a peut-être été fait. Vérifie avant de réessayer.";
  const SILENT = "Le serveur Mementum ne répond pas. Vérifie qu'il est lancé, puis réessaie.";
  const failed = async (call: Promise<unknown>) => call.then(() => null, (error: unknown) => error as InstanceType<typeof api.ApiError>);
  // Une écriture : coupée en route, ou rendue par un mandataire sans corps de l'API.
  const ways: ((call: Received) => void)[] = [(c) => c.fail(), (c) => c.answerLostBody(502), (c) => c.answerLostBody(504)];
  for (const way of ways) {
    const creating = failed(api.createPage({ spaceId: "s1", section: "space" }));
    way(await waiting("POST", "/api/v1/pages"));
    const error = (await creating)!;
    expect(error.message).toBe(MAYBE);
    expect(api.answerLost(error)).toBe(true);
    const reading = failed(api.getTree("s1"));
    way(await waiting("GET", "/api/v1/spaces/s1/tree"));
    const silent = (await reading)!;
    expect(silent.message).toBe(SILENT);
    expect(api.answerLost(silent)).toBe(true);
  }
  // Une réponse dont le corps se perd après un `201` : l'écriture est faite, la réponse non lue.
  const uploading = failed(api.copyFile("p1", { pageId: "p2", fileId: "f1" }));
  (await waiting("POST", "/api/v1/pages/p1/files/copies")).answerLostBody(201);
  expect((await uploading)!.message).toBe(MAYBE);
  // Le serveur lui-même répond (une erreur de l'API, `500 internal` compris) : ce n'est pas une réponse perdue.
  const refused = failed(api.createPage({ spaceId: "s1", section: "space" }));
  (await waiting("POST", "/api/v1/pages")).answer(500, { error: "internal", message: "erreur interne du serveur" });
  const internal = (await refused)!;
  expect(internal.message).toBe("erreur interne du serveur");
  expect(api.answerLost(internal)).toBe(false);
  expect(api.answerLost(new Error("autre chose"))).toBe(false);
  // Se connecter ou se déconnecter n'écrit rien qu'on puisse refaire à tort : le serveur ne répond pas.
  const leaving = failed(api.logOut());
  (await waiting("POST", "/api/v1/auth/logout")).fail();
  expect((await leaving)!.message).toBe(SILENT);
  expect(api.lostAnswer("la page a peut-être été créée", "Regarde dans la barre latérale avant de réessayer.")).toBe(
    "La réponse du serveur n'est pas arrivée : la page a peut-être été créée. Regarde dans la barre latérale avant de réessayer.",
  );
});

// La recherche (lot 4, Task C1) : un `POST` qui ne fait que lire (R917). La recherche est dans le
// corps, jamais dans l'adresse ; une réponse perdue se dit comme celle d'une lecture (R757).

const HITS = { results: [], more: false, indexing: false };

test("the_query_is_never_put_in_an_address", async () => {
  await loadedAs("alice");
  const WORD = "témoin-secret";
  const first = api.searchSpace("s1", { q: `${WORD} & co ?` });
  const call = await waiting("POST", "/api/v1/spaces/s1/search");
  // L'adresse ne porte que l'espace ; la recherche part dans le corps, telle quelle.
  expect(call.path).toBe("/api/v1/spaces/s1/search");
  expect(call.body).toEqual({ q: `${WORD} & co ?` });
  expect(call.type).toBe("application/json");
  expect(call.account).toBe("alice");
  call.answer(200, HITS);
  await expect(first).resolves.toEqual(HITS);
  // La suite d'une recherche : `after` dans le corps aussi.
  const next = api.searchSpace("s1", { q: WORD, after: 20 });
  const second = await waiting("POST", "/api/v1/spaces/s1/search");
  expect(second.body).toEqual({ q: WORD, after: 20 });
  second.answer(200, HITS);
  await next;
  // Aucune adresse appelée, aucune fois, ne porte le mot.
  for (const received of log()) expect(received).not.toContain("moin");
  // Un espace au nom étrange est échappé dans le chemin.
  void api.searchSpace("a/b?c", { q: WORD }).catch(() => {});
  await waiting("POST", "/api/v1/spaces/a%2Fb%3Fc/search");
});

test("a_lost_answer_of_a_search_is_said_as_a_reading", async () => {
  await loadedAs("alice");
  const SILENT = "Le serveur Mementum ne répond pas. Vérifie qu'il est lancé, puis réessaie.";
  const failed = async (call: Promise<unknown>) =>
    call.then(
      () => null,
      (error: unknown) => error as InstanceType<typeof api.ApiError>,
    );
  // Coupée en route, rendue par un mandataire sans corps de l'API, ou le corps perdu après les
  // en-têtes : le serveur ne répond pas, et rien n'« a peut-être été fait ».
  const ways: ((call: Received) => void)[] = [
    (c) => c.fail(),
    (c) => c.answerLostBody(502),
    (c) => c.answerLostBody(504),
    (c) => c.answerLostBody(200),
  ];
  for (const way of ways) {
    const searching = failed(api.searchSpace("s1", { q: "chat" }));
    way(await waiting("POST", "/api/v1/spaces/s1/search"));
    const error = (await searching)!;
    expect(error.message).toBe(SILENT);
    expect(error.message).not.toContain("peut-être");
  }
  // Témoin : une écriture perdue de la même façon dit toujours le doute, et un autre `POST` d'un
  // espace n'est pas pris pour une lecture.
  const creating = failed(api.createPage({ spaceId: "s1", section: "space" }));
  (await waiting("POST", "/api/v1/pages")).fail();
  expect((await creating)!.message).toContain("peut-être été fait");
  const inviting = failed(api.inviteToSpace("s1", { email: "bruno@exemple.fr", role: "member" }));
  (await waiting("POST", "/api/v1/spaces/s1/invitations")).fail();
  expect((await inviting)!.message).toContain("peut-être été fait");
  // Un refus du serveur est rendu tel qu'il le dit.
  const refused = failed(api.searchSpace("s1", { q: "chat" }));
  (await waiting("POST", "/api/v1/spaces/s1/search")).answer(422, {
    error: "invalid_query",
    message: "ta recherche est trop longue",
  });
  expect(await refused).toMatchObject({ status: 422, code: "invalid_query", message: "ta recherche est trop longue" });
});

test("a_lost_answer_of_an_invitation_lookup_is_said_as_a_reading", async () => {
  // R1204 : lire ce que dit un lien d'invitation n'écrit rien (R1092) ; sa réponse perdue ne dit
  // donc pas « ce que tu as demandé a peut-être été fait ».
  await loadedAs("alice");
  const SILENT = "Le serveur Mementum ne répond pas. Vérifie qu'il est lancé, puis réessaie.";
  const failed = async (call: Promise<unknown>) =>
    call.then(
      () => null,
      (error: unknown) => error as InstanceType<typeof api.ApiError>,
    );
  // Coupée en route, ou rendue par un mandataire sans corps de l'API.
  const ways: ((call: Received) => void)[] = [(c) => c.fail(), (c) => c.answerLostBody(502), (c) => c.answerLostBody(504)];
  for (const way of ways) {
    const looking = failed(api.lookupInvitation("jeton"));
    way(await waiting("POST", "/api/v1/invitations/lookup"));
    const error = (await looking)!;
    expect(error.message).toBe(SILENT);
    expect(error.message).not.toContain("peut-être");
  }
  // Témoin : accepter l'invitation écrit, et sa réponse perdue dit toujours le doute.
  const accepting = failed(api.acceptInvitation("jeton"));
  (await waiting("POST", "/api/v1/invitations/accept")).fail();
  expect((await accepting)!.message).toContain("peut-être été fait");
});

test("a_search_without_session_asks_to_log_in_and_a_late_refusal_sends_it_again", async () => {
  await loadedAs("alice");
  // Sous la session en cours : la connexion est demandée, la recherche ne repart pas d'elle-même.
  const lost = api.searchSpace("s1", { q: "chat" });
  (await waiting("POST", "/api/v1/spaces/s1/search")).answer(401, UNAUTHENTICATED);
  await expect(lost).rejects.toMatchObject({ code: "unauthenticated" });
  await flush();
  expect(asked).toBe(1);
  expect(count("POST", "/api/v1/spaces/s1/search")).toBe(1);
  // Partie avant une session rouverte, refusée après : elle repart une fois, comme une lecture.
  const late = api.searchSpace("s1", { q: "chien" });
  const first = await waiting("POST", "/api/v1/spaces/s1/search");
  await logIn("alice");
  first.answer(401, UNAUTHENTICATED);
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  const again = await waiting("POST", "/api/v1/spaces/s1/search");
  expect(again.body).toEqual({ q: "chien" });
  again.answer(200, HITS);
  await expect(late).resolves.toEqual(HITS);
  expect(asked).toBe(1);
});
