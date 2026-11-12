// Présence : ce qu'annonce une connexion en lecture seule, ou un client qui n'est pas le
// nôtre, n'atteint l'éditeur de personne (Rulings R118, R119, R123).
//
// Le serveur relayait tout état de présence qui était du JSON. Or le client pose la couleur
// d'un curseur telle quelle dans un attribut `style` (BlockNote), et décode la position d'un
// curseur sans garde (y-prosemirror) : une couleur piégée injectait du CSS chez tous ceux qui
// écrivent, et un curseur mal formé levait une exception à chaque changement venu du réseau.
// Une invitée en lecture suffisait.
//
// Ces tests parlent au serveur par une WebSocket brute, avec la session d'un compte, comme le
// ferait un client écrit pour l'occasion ; ce qu'ils vérifient se lit dans les navigateurs de
// deux personnes qui ont la page ouverte et qui écrivent.
//
// Le navigateur a aussi sa propre garde (`src/editor/presenceGuard.ts`) : le premier test lui
// fait recevoir un état mal formé sans passer par le serveur.
import { expect, test, type APIRequestContext, type Browser, type Page } from "@playwright/test";
// Le type de `window.__mementum`.
import type {} from "../src/editor/debug";
import {
  apiAs,
  closeContexts,
  createPage,
  getMe,
  getMmd,
  guestOf,
  memberOf,
  openPage,
  ownerAndMember,
  REGISTRY_VERSION,
  setPageLevel,
  unique,
} from "./helpers";

test.afterEach(closeContexts);

/** Le nom du cookie de session du serveur. */
const SESSION_COOKIE = "mementum_session";

/** Une couleur qui n'en est pas une : la suite de l'attribut `style` où elle serait posée. */
const TRAPPED_COLOUR = "red;position:fixed;inset:0;z-index:99999";
/**
 * Deux couleurs en règle, hors de la palette de l'application (`PRESENCE_COLORS`) : un curseur
 * qui les porte ne peut pas être celui de Mona ou de Tom.
 */
const GINA_COLOUR = "#0A7F7F";
const EVE_COLOUR = "#7F0A7F";
/** La fin du document, en position relative de Yjs : elle ne demande aucun identifiant. */
const END_OF_DOCUMENT = { tname: "document-store", assoc: 0 };
const VALID_CURSOR = { anchor: END_OF_DOCUMENT, head: END_OF_DOCUMENT };

/** Un entier variable de lib0 (7 bits par octet, poids faibles d'abord). */
function varUint(value: number): number[] {
  const bytes: number[] = [];
  let rest = value;
  while (rest > 0x7f) {
    bytes.push(0x80 | (rest % 0x80));
    rest = Math.floor(rest / 0x80);
  }
  bytes.push(rest);
  return bytes;
}

/** Une entrée de présence : client, horloge, état en JSON. */
function entry(client: number, clock: number, state: unknown): number[] {
  const json = [...new TextEncoder().encode(JSON.stringify(state))];
  return [...varUint(client), ...varUint(clock), ...varUint(json.length), ...json];
}

/**
 * Un message de présence de y-websocket : `count` est le nombre d'entrées tel qu'il est
 * écrit (par défaut, le vrai, en entier variable).
 */
function presence(
  entries: number[][],
  count: number[] = varUint(entries.length),
): Uint8Array<ArrayBuffer> {
  const payload = [...count, ...entries.flat()];
  return Uint8Array.from([1, ...varUint(payload.length), ...payload]);
}

/** Une connexion de synchro brute : ce qu'on y écrit part tel quel. */
type RawSync = {
  send(bytes: Uint8Array<ArrayBuffer>): void;
  /**
   * Attend que le serveur ait traité tout ce qui a été envoyé, ou qu'il ait fermé la
   * connexion, et dit lequel.
   */
  settled(): Promise<"servie" | "fermée">;
  /** Le serveur a traité tout ce qui a été envoyé, et la connexion est encore ouverte. */
  processed(): Promise<void>;
  close(): void;
};

/**
 * Ouvre `/sync/{pageId}` avec la session de `api`. Une WebSocket ouverte par Node ne joint pas
 * le cookie d'elle-même : il est posé dans les en-têtes (comme `webSocketWithSession` de
 * `helpers.ts`).
 */
async function rawSync(api: APIRequestContext, baseURL: string, pageId: string): Promise<RawSync> {
  const { cookies } = await api.storageState();
  const session = cookies.find((cookie) => cookie.name === SESSION_COOKIE);
  if (!session) throw new Error("ce client de l'API n'a pas de session");
  const url = `${baseURL.replace(/^http/, "ws")}/sync/${pageId}?schema=${REGISTRY_VERSION}`;
  // Le second argument de la WebSocket de Node (undici) admet des en-têtes ; le type du DOM,
  // lui, ne connaît que les sous-protocoles.
  const options = { headers: { cookie: `${session.name}=${session.value}` } };
  const socket = new WebSocket(url, options as unknown as string[]);
  socket.binaryType = "arraybuffer";
  let answers = 0;
  let closed = false;
  socket.addEventListener("message", (event) => {
    const bytes = new Uint8Array(event.data as ArrayBuffer);
    // Une étape 2 : la réponse du serveur à une étape 1.
    if (bytes[0] === 0 && bytes[1] === 1) answers += 1;
  });
  socket.addEventListener("close", () => {
    closed = true;
  });
  await new Promise<void>((resolve, reject) => {
    socket.addEventListener("open", () => resolve());
    socket.addEventListener("error", () => reject(new Error(`connexion refusée : ${url}`)));
  });
  const settled = async (): Promise<"servie" | "fermée"> => {
    const before = answers;
    // Une étape 1 au vecteur d'état vide : le serveur sert une connexion dans l'ordre, et sa
    // réponse dit que tout ce qui précède est traité.
    if (!closed) socket.send(Uint8Array.from([0, 0, 1, 0]));
    await expect.poll(() => closed || answers > before, { timeout: 10_000 }).toBe(true);
    return closed ? "fermée" : "servie";
  };
  return {
    send: (bytes) => socket.send(bytes),
    settled,
    async processed() {
      expect(await settled()).toBe("servie");
    },
    close: () => socket.close(),
  };
}

/** Les erreurs levées dans la page, à partir de maintenant. */
function pageErrors(p: Page): string[] {
  const errors: string[] = [];
  p.on("pageerror", (error) => errors.push(error.message));
  return errors;
}

/** Les éléments de la page dont l'attribut `style` porte la couleur piégée. */
function trapped(p: Page) {
  return p.locator('[style*="99999"]');
}

/**
 * Tape `text` à la fin du document de `from`, et attend qu'il arrive dans l'éditeur de `to`.
 * Tout ce que le serveur a relayé à `to` avant cette frappe y est alors arrivé aussi : la
 * diffusion d'une page est ordonnée.
 */
async function typeAndSee(from: Page, to: Page, text: string): Promise<void> {
  await from.locator(".bn-editor").click();
  await from.keyboard.press("Control+End");
  await from.keyboard.type(text);
  await expect(to.locator(".bn-editor")).toContainText(text, { timeout: 10_000 });
}

/**
 * La scène : une page que Mona et Tom ont ouverte, et où ils écrivent tous les deux ; et une
 * troisième personne, qui parlera au serveur par une WebSocket brute : une invitée en lecture,
 * ou une membre qui écrit.
 *
 * Ses droits sont posés avant que les pages s'ouvrent : un changement d'accès ferme les
 * connexions de synchro de la page, et la présence de chacun met ensuite un moment à revenir.
 */
async function twoWriters(browser: Browser, third: "invitée en lecture" | "membre qui écrit") {
  const { alice: mona, aliceApi: monaApi, space, bruno: tom, brunoId: tomId } =
    await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Présence ${unique()}` });
  await setPageLevel(monaApi, page.id, tomId, "edit");
  let thirdApi: APIRequestContext;
  if (third === "invitée en lecture") {
    thirdApi = await apiAs(await guestOf(mona, page.id, `Gina ${unique()}`, "read"));
  } else {
    thirdApi = await apiAs(await memberOf(mona, space.id, `Ève ${unique()}`));
    await setPageLevel(monaApi, page.id, (await getMe(thirdApi)).user.id, "edit");
  }
  const monaPage = await openPage(browser, page.id, mona);
  const tomPage = await openPage(browser, page.id, tom);
  const errors = { mona: pageErrors(monaPage), tom: pageErrors(tomPage) };
  // Les deux éditeurs marchent, dans les deux sens, avant que quiconque s'en mêle.
  await typeAndSee(tomPage, monaPage, "Tom écrit. ");
  await typeAndSee(monaPage, tomPage, "Mona écrit. ");
  return { mona, monaApi, thirdApi, page, monaPage, tomPage, errors };
}

/**
 * Ce qui doit rester vrai après ce qu'un client a annoncé : aucun élément ne porte la couleur
 * piégée, aucune erreur dans les pages, et chacun reçoit encore ce que l'autre tape, sans que
 * rien de ce qui était écrit soit perdu.
 */
async function editorsStillWork(scene: Awaited<ReturnType<typeof twoWriters>>): Promise<void> {
  const { monaApi, page, monaPage, tomPage, errors } = scene;
  await typeAndSee(tomPage, monaPage, "Tom écrit encore. ");
  await typeAndSee(monaPage, tomPage, "Mona écrit encore. ");
  expect.soft(await trapped(monaPage).count(), "couleur piégée chez Mona").toBe(0);
  expect.soft(await trapped(tomPage).count(), "couleur piégée chez Tom").toBe(0);
  expect.soft(errors.mona, "erreurs dans la page de Mona").toEqual([]);
  expect.soft(errors.tom, "erreurs dans la page de Tom").toEqual([]);
  // Rien de perdu : le serveur a les quatre frappes.
  const mmd = await getMmd(monaApi, page.id);
  for (const text of ["Tom écrit.", "Mona écrit.", "Tom écrit encore.", "Mona écrit encore."]) {
    expect.soft(mmd, `« ${text} » sur le serveur`).toContain(text);
  }
}

/**
 * Fait recevoir `bytes` à l'onglet `p`, comme si le serveur les lui avait envoyés : le message
 * est donné à y-websocket, sans passer par le serveur ni par ses règles. Dans une tâche à
 * part, comme un vrai message : une exception levée en le traitant est alors une erreur de la
 * page, pas un rejet de cet appel.
 */
async function receiveFromServer(p: Page, bytes: Uint8Array<ArrayBuffer>): Promise<void> {
  await p.evaluate((numbers) => {
    const socket = window.__mementum!.provider.ws!;
    const data = Uint8Array.from(numbers).buffer;
    setTimeout(() => socket.onmessage!({ data } as MessageEvent), 0);
  }, [...bytes]);
}

test("malformed_presence_that_reaches_a_browser_is_not_rendered", async ({ browser }) => {
  // Le serveur ne relaie plus un état mal formé (les deux tests suivants). Ici, c'est le
  // navigateur seul qui est mis à l'épreuve : un serveur plus ancien, ou une règle du serveur
  // qui lirait un état autrement que le client, et l'état lui arrive quand même.
  const scene = await twoWriters(browser, "invitée en lecture");
  const user = { name: "Olga", color: "#B4462B" };
  const forged = presence([
    // Une couleur piégée, avec un curseur valide : il serait affiché, et la couleur posée
    // telle quelle dans un attribut `style`.
    entry(720_001, 1, { user: { name: "Olga", color: TRAPPED_COLOUR }, cursor: VALID_CURSOR }),
    // Un curseur mal formé : sa lecture lève une exception dans y-prosemirror.
    entry(720_002, 1, { user, cursor: { anchor: 7, head: 7 } }),
    // Un `user` qui n'est pas un objet : y-prosemirror y écrit une couleur par défaut.
    entry(720_003, 1, { user: "Olga", cursor: VALID_CURSOR }),
    // Et un état en règle : lui s'affiche.
    entry(720_004, 1, { user: { name: "Ève", color: EVE_COLOUR }, cursor: VALID_CURSOR }),
  ]);
  for (const p of [scene.monaPage, scene.tomPage]) await receiveFromServer(p, forged);

  await editorsStillWork(scene);
  for (const p of [scene.monaPage, scene.tomPage]) {
    await expect(p.locator(`.bn-collaboration-cursor__caret[style*="${EVE_COLOUR}"]`)).toHaveCount(1);
  }
});

test("presence_of_a_reader_reaches_no_editor", async ({ browser, baseURL }) => {
  // Gina, invitée en lecture sur la page.
  const scene = await twoWriters(browser, "invitée en lecture");
  const socket = await rawSync(scene.thirdApi, baseURL!, scene.page.id);
  try {
    // Une couleur piégée, avec un curseur valide : il serait affiché.
    socket.send(
      presence([entry(700_001, 1, { user: { name: "Olga", color: TRAPPED_COLOUR }, cursor: VALID_CURSOR })]),
    );
    // Un curseur mal formé : sa lecture lèverait une exception chez qui le reçoit.
    socket.send(
      presence([
        entry(700_002, 1, { user: { name: "Olga", color: "#B4462B" }, cursor: { anchor: 7, head: 7 } }),
      ]),
    );
    // Et un état en règle : en lecture seule, rien n'est annoncé non plus.
    socket.send(
      presence([entry(700_003, 1, { user: { name: "Gina", color: GINA_COLOUR }, cursor: VALID_CURSOR })]),
    );
    await socket.processed();

    await editorsStillWork(scene);
    await expect(
      scene.monaPage.locator(`.bn-collaboration-cursor__caret[style*="${GINA_COLOUR}"]`),
    ).toHaveCount(0);
    // La connexion de Gina n'est pas fermée pour autant.
    await socket.processed();
  } finally {
    socket.close();
  }
});

test("malformed_presence_of_a_writer_reaches_no_editor", async ({ browser, baseURL }) => {
  // Ève, membre de l'espace, qui écrit dans la page.
  const scene = await twoWriters(browser, "membre qui écrit");
  const eveApi = scene.thirdApi;
  const socket = await rawSync(eveApi, baseURL!, scene.page.id);
  // Le client de Mona, et le nom sous lequel Tom la voit.
  const monaClient = await scene.monaPage.evaluate(() => window.__mementum!.doc.clientID);
  const monaSeenByTom = () =>
    scene.tomPage.evaluate((client) => {
      const state = window.__mementum!.provider.awareness.getStates().get(client);
      return (state as { user?: { name?: string } } | undefined)?.user?.name;
    }, monaClient);
  await expect.poll(monaSeenByTom).toBe(scene.mona.name);
  try {
    const user = { name: "Ève", color: "#B4462B" };
    const nothing = { type: null, tname: null, item: null };
    const malformed: unknown[] = [
      { user: { name: "Ève", color: TRAPPED_COLOUR }, cursor: VALID_CURSOR },
      { user, cursor: { anchor: 7, head: 7 } },
      { user, cursor: { anchor: nothing, head: nothing } },
      { user, cursor: VALID_CURSOR, surprise: true },
      { user: { name: "Ève", color: 42 }, cursor: VALID_CURSOR },
    ];
    malformed.forEach((state, index) => socket.send(presence([entry(710_001 + index, 1, state)])));
    // Ces états sont ignorés, sans fermer la connexion.
    await socket.processed();

    // D'une autre connexion d'Ève, un message dont le nombre d'entrées est écrit de sorte que
    // le serveur en lise une et un client deux (`81 80 80 80 10` : 1 sur 32 bits, 2^32 + 1
    // sinon). La seconde entrée vise le client de Mona. Le serveur peut fermer cette
    // connexion : ce message-là est illisible.
    const forger = await rawSync(eveApi, baseURL!, scene.page.id);
    forger.send(
      presence(
        [
          entry(710_010, 1, { user }),
          entry(monaClient, 1_000_000, { user: { name: "Mona (faux)", color: "#B4462B" } }),
        ],
        [0x81, 0x80, 0x80, 0x80, 0x10],
      ),
    );
    await forger.settled();
    forger.close();

    // Un état en règle, d'une troisième connexion d'Ève : lui s'affiche, chez Mona comme chez
    // Tom.
    const again = await rawSync(eveApi, baseURL!, scene.page.id);
    try {
      again.send(
        presence([entry(710_020, 1, { user: { name: "Ève", color: EVE_COLOUR }, cursor: VALID_CURSOR })]),
      );
      await again.processed();
      await editorsStillWork(scene);
      for (const p of [scene.monaPage, scene.tomPage]) {
        await expect(p.locator(`.bn-collaboration-cursor__caret[style*="${EVE_COLOUR}"]`)).toHaveCount(1);
      }
      // Mona est toujours Mona pour Tom.
      expect.soft(await monaSeenByTom(), "le nom de Mona, vu de Tom").toBe(scene.mona.name);
    } finally {
      again.close();
    }
  } finally {
    socket.close();
  }
});
