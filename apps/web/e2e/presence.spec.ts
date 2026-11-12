// Qui regarde la page : les avatars de la barre du haut (liste des présents du serveur, R136),
// le nom d'un curseur, et le retour de la présence après une coupure.
import { expect, test, type Browser, type Locator, type Page } from "@playwright/test";
// Le type de `window.__mementum`.
import type {} from "../src/editor/debug";
import {
  apiAs,
  closeContexts,
  createPage,
  getMe,
  guestOf,
  holdPageReads,
  memberOf,
  openPage,
  newUser,
  ownerAndMember,
  setPageLevel,
  unique,
  waitSynced,
  type Account,
} from "./helpers";
import { rawSync, varUint } from "./raw-sync";

test.afterEach(closeContexts);

/** Les avatars des autres présents, dans la barre du haut de `p`. */
function avatars(p: Page): Locator {
  return p.locator(".mm-topbar .mm-presence .mm-avatar");
}

/** Ce que y-websocket écrit dans la console quand il reçoit un type de message qu'il ne connaît pas. */
const UNKNOWN_MESSAGE = "Unable to compute message";

/**
 * Ouvre la page avec la session de `account`, en écoutant la console **avant** la navigation :
 * la première trame de page arrive pendant l'ouverture, et c'est souvent la seule.
 */
async function openWatched(browser: Browser, id: string, account: Account) {
  const p = await newUser(browser, account);
  const lines: string[] = [];
  p.on("console", (message) => lines.push(message.text()));
  await p.goto(`/p/${id}`);
  await waitSynced(p);
  return { page: p, lines };
}

/** Une page que Mona possède, où Tom (membre) écrit. */
async function scene() {
  const {
    alice: mona,
    aliceApi: monaApi,
    space,
    bruno: tom,
    brunoId: tomId,
  } = await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Regard ${unique()}` });
  await setPageLevel(monaApi, page.id, tomId, "edit");
  return { mona, monaApi, space, tom, page };
}

test("everyone_on_the_page_is_shown_in_the_top_bar", async ({ browser }) => {
  const { mona, tom, page } = await scene();
  // Gina, invitée en lecture : elle lit la page, et figure aussi dans la liste.
  const gina = await guestOf(mona, page.id, `Gina ${unique()}`, "read");
  const { page: monaPage, lines: monaConsole } = await openWatched(browser, page.id, mona);
  const { page: tomPage, lines: tomConsole } = await openWatched(browser, page.id, tom);
  const { page: ginaPage, lines: ginaConsole } = await openWatched(browser, page.id, gina);

  // Mona voit Tom et Gina (« lecture »), pas elle-même.
  await expect(avatars(monaPage)).toHaveCount(2);
  const tomAvatar = monaPage.getByRole("img", { name: tom.name, exact: true });
  await expect(tomAvatar).toBeVisible();
  await expect(
    monaPage.getByRole("img", { name: `${gina.name} (lecture)`, exact: true }),
  ).toBeVisible();
  await expect(monaPage.getByRole("img", { name: mona.name })).toHaveCount(0);
  // L'initiale, sur la couleur du compte, et le nom en infobulle.
  await expect(tomAvatar).toHaveText(tom.name[0].toUpperCase());
  await expect(tomAvatar).toHaveAttribute("title", tom.name);
  // Tom voit Mona et Gina ; Gina, qui lit, voit les deux autres.
  await expect(avatars(tomPage)).toHaveCount(2);
  await expect(avatars(ginaPage)).toHaveCount(2);
  await expect(ginaPage.getByRole("img", { name: mona.name, exact: true })).toBeVisible();
  // Les avatars sont à gauche de « Partager » (Mona a l'accès complet).
  const share = monaPage.getByRole("button", { name: "Partager" });
  await expect(share).toBeVisible();
  const avatarBox = await avatars(monaPage).last().boundingBox();
  const shareBox = await share.boundingBox();
  expect(avatarBox!.x + avatarBox!.width).toBeLessThanOrEqual(shareBox!.x);

  // Deux onglets du même compte : l'autre onglet de Mona n'est pas un avatar chez elle.
  const monaAgain = await openPage(browser, page.id, mona);
  await expect(avatars(monaAgain)).toHaveCount(2);
  await expect(avatars(monaPage)).toHaveCount(2);

  // La trame de page n'a jamais atteint y-websocket.
  for (const lines of [monaConsole, tomConsole, ginaConsole]) {
    expect(lines.filter((line) => line.includes(UNKNOWN_MESSAGE))).toEqual([]);
  }
});

test("leaving_the_page_removes_the_avatar", async ({ browser }) => {
  const { mona, tom, page } = await scene();
  const monaPage = await openPage(browser, page.id, mona);
  const tomPage = await openPage(browser, page.id, tom);
  await expect(avatars(monaPage)).toHaveCount(1);
  await expect(avatars(monaPage)).toHaveAttribute("aria-label", tom.name);

  await tomPage.close();
  await expect(avatars(monaPage)).toHaveCount(0);
  // Il revient : l'avatar aussi.
  const back = await openPage(browser, page.id, tom);
  await expect(avatars(monaPage)).toHaveCount(1);
  await expect(avatars(back)).toHaveCount(1);
});

/** Une entrée de présence : client, horloge, état en JSON. */
function entry(client: number, clock: number, state: unknown): number[] {
  const json = [...new TextEncoder().encode(JSON.stringify(state))];
  return [...varUint(client), ...varUint(clock), ...varUint(json.length), ...json];
}

test("cursor_label_is_the_account_name", async ({ browser, baseURL }) => {
  const { mona, monaApi, space, page } = await scene();
  const eve: Account = await memberOf(mona, space.id, `Ève ${unique()}`);
  const eveApi = await apiAs(eve);
  await setPageLevel(monaApi, page.id, (await getMe(eveApi)).user.id, "edit");
  const monaPage = await openPage(browser, page.id, mona);
  const colour = "#0A7F7F";
  const end = { tname: "document-store", assoc: 0 };
  const socket = await rawSync(eveApi, baseURL!, page.id);
  try {
    // Un client brut annonce le nom d'une autre personne.
    const payload = [
      ...varUint(1),
      ...entry(730_001, 1, {
        user: { name: "Mona (faux nom)", color: colour },
        cursor: { anchor: end, head: end },
      }),
    ];
    socket.send(Uint8Array.from([1, ...varUint(payload.length), ...payload]));
    await socket.processed();
    // L'étiquette du curseur porte le nom du compte d'Ève, celui que le serveur a posé.
    const label = monaPage.locator(
      `.bn-collaboration-cursor__caret[style*="${colour}"] .bn-collaboration-cursor__label`,
    );
    await expect(label).toHaveText(eve.name);
    await expect(monaPage.getByText("faux nom")).toHaveCount(0);
  } finally {
    socket.close();
  }
});

test("cursor_returns_at_once_after_a_reconnect", async ({ browser }) => {
  const { mona, tom, page } = await scene();
  const monaPage = await openPage(browser, page.id, mona);
  const tomPage = await openPage(browser, page.id, tom);
  // Tom pose son curseur dans le texte, puis ne tape plus.
  await tomPage.locator(".bn-editor").click();
  const cursor = monaPage.locator(".bn-collaboration-cursor__label", { hasText: tom.name });
  await expect(cursor).toHaveCount(1);

  // Sa connexion tombe, et sa reconnexion est retenue : son curseur disparaît chez Mona.
  const held = await holdPageReads(tomPage, page.id);
  const clock = () =>
    tomPage.evaluate(() => {
      const { awareness, doc } = window.__mementum!.provider;
      return awareness.meta.get(doc.clientID)!.clock;
    });
  const clockBefore = await clock();
  await tomPage.evaluate(() => window.__mementum!.provider.ws!.close());
  await held.asked;
  await expect(cursor).toHaveCount(0);

  // Il revient sans rien taper : le curseur est de nouveau là en moins de trois secondes.
  held.release();
  await expect(cursor).toHaveCount(1, { timeout: 3_000 });
  expect(await clock()).toBeGreaterThan(clockBefore);
});
