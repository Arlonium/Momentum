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
  answered: boolean;
  answer(status: number, body?: unknown): void;
  /** Pas de réponse : le réseau est coupé. */
  fail(): void;
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
const PASSWORDS = { current: "ancien-mot-de-passe", new: "nouveau-mot-de-passe" };
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
          answered: false,
          answer(status, body) {
            call.answered = true;
            resolve(new Response(body === undefined ? null : JSON.stringify(body), { status }));
          },
          fail() {
            call.answered = true;
            reject(new TypeError("Failed to fetch"));
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
  (await waiting("POST", "/api/v1/auth/password")).answer(204);
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
  password.answer(204);
  await change;
  (await waiting("GET", "/api/v1/me")).answer(200, me("alice"));
  (await waiting("GET", "/api/v1/spaces/s1/tree")).answer(200, tree("s1"));
  await expect(read).resolves.toEqual(tree("s1"));
  expect(asked).toBe(0);
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
