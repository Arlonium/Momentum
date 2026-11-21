// Les droits de la recherche, de bout en bout (lot 4, Task X1) : personne ne trouve, ne lit en
// extrait, en titre ni en fil d'Ariane une page qu'il ne lit pas (page privée d'un autre, page d'un
// autre espace, corbeille, accès retiré, invité limité à ses pages), et les gestes de la fenêtre
// tiennent devant un droit qui change.
//
// Trois règles d'écriture :
// - un mot à chercher n'est porté que par le **texte** d'une page (`word()`), jamais par son titre :
//   une ligne trouvée par le titre vient de l'arbre de l'onglet (`data-from="tree"`), et ne dit rien
//   de la route ; on juge les lignes `data-from="server"` et le corps de la réponse ;
// - une absence se juge derrière la réponse du serveur à cette recherche (`ask`), et ce que l'écran
//   en dit (« Aucun résultat. ») ;
// - l'index suit une écriture après un temps de calme : `waitFindable` avant d'ouvrir les onglets, et
//   jamais une recherche par l'API pendant que l'onglet du même compte cherche ;
// - une réponse prise pour référence (« un mot que personne n'a écrit »), puis comparée octet pour
//   octet, se prend derrière `waitIndexed`, après la dernière écriture : `indexing` reste vrai une
//   seconde après une indexation, et passerait à faux entre la référence et les réponses comparées.
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import type { SearchResults } from "@mementum/protocol";
import {
  acceptInvitation,
  accessLost,
  apiAs,
  closeContexts,
  closeEventsChannel,
  createPage,
  expectSignedInAs,
  firstSpace,
  getMe,
  getMmd,
  guestOf,
  hideSyncClosures,
  inviteToPage,
  joinSpace,
  logInWith,
  memberOf,
  newUser,
  NO_SESSION,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  searchIn,
  setPageLevel,
  signUpAccount,
  spaceSwitcher,
  titleOf,
  unique,
  UNKNOWN_ID,
  waitFindable,
  waitIndexed,
  waitSynced,
  word,
} from "./helpers";

test.afterEach(closeContexts);

const entry = (p: Page) => p.locator("[data-search-entry]");
const win = (p: Page) => p.getByRole("dialog", { name: "Rechercher" });
const field = (p: Page) => win(p).getByRole("combobox", { name: "Rechercher dans l'espace" });
const row = (p: Page, id: string) => win(p).locator(`[data-search-page="${id}"]`);
const nothingFound = (p: Page) => win(p).getByRole("status").filter({ hasText: "Aucun résultat." });
/** Les pages des lignes de la fenêtre, de haut en bas, avec leur provenance. */
const shown = (p: Page) =>
  win(p)
    .locator("[data-search-page]")
    .evaluateAll((rows) =>
      rows.map((r) => `${(r as HTMLElement).dataset.from}:${(r as HTMLElement).dataset.searchPage}`),
    );
const focusOnBody = (p: Page) => p.evaluate(() => document.activeElement === document.body);
const SEARCH = /^\/api\/v1\/spaces\/[^/]+\/search$/;

type Answer = { status: number; text: string; pages: string[] };

/**
 * Tape `query` dans la fenêtre ouverte, et attend la réponse du serveur à **cette** recherche : la
 * barrière de toute absence. Rend son statut, son corps tel quel, et les pages qu'il nomme.
 */
async function ask(p: Page, query: string): Promise<Answer> {
  const answered = p.waitForResponse((res) => {
    const request = res.request();
    if (request.method() !== "POST" || !SEARCH.test(new URL(res.url()).pathname)) return false;
    return (request.postDataJSON() as { q?: string } | null)?.q === query;
  });
  await field(p).fill(query);
  const res = await answered;
  const text = await res.text();
  const pages = res.status() === 200 ? (JSON.parse(text) as SearchResults).results.map((hit) => hit.pageId) : [];
  return { status: res.status(), text, pages };
}

/** Ouvre la fenêtre par son entrée de la barre latérale. */
async function openWindow(p: Page) {
  await entry(p).click();
  await expect(field(p)).toBeFocused();
}

/** Les pages que le serveur trouve pour `query`, triées. */
async function found(api: APIRequestContext, spaceId: string, query: string): Promise<string[]> {
  return (await searchIn(api, spaceId, query)).results.map((hit) => hit.pageId).sort();
}

const sorted = (ids: string[]) => [...ids].sort();

test("a_guest_never_finds_a_page_he_does_not_see", async ({ browser }) => {
  const tag = unique();
  const alice = await signUpAccount(`Alice ${tag}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const bruno = await memberOf(alice, space.id, `Bruno ${tag}`);
  const brunoApi = await apiAs(bruno);
  // Un mot témoin par page, et un mot que toutes portent ; aucun titre n'en porte.
  const w = { open: word(), priv: word(), guest: word(), sub: word(), deep: word(), trash: word() };
  const all = word();
  const hiddenTitle = word();
  const text = (own: string) => `Le ${own} est écrit ici, avec le ${all}.\n`;
  const open = await createPage(aliceApi, { title: `Ouverte ${tag}` });
  const priv = await createPage(aliceApi, { title: `Privée ${tag}`, section: "private" });
  const invited = await createPage(aliceApi, { title: `Invitée ${tag}` });
  const sub = await createPage(aliceApi, { title: `Sous-page ${tag}`, parentId: invited.id });
  // Une page que l'invité ne voit pas, dont il voit une sous-page : son titre ne doit paraître
  // dans aucun fil d'Ariane.
  const hidden = await createPage(aliceApi, { title: `Parent ${hiddenTitle}` });
  const deep = await createPage(aliceApi, { title: `Profonde ${tag}`, parentId: hidden.id });
  const trashed = await createPage(aliceApi, { title: `Jetée ${tag}` });
  await putMmd(aliceApi, open.id, text(w.open));
  await putMmd(aliceApi, priv.id, text(w.priv));
  await putMmd(aliceApi, invited.id, text(w.guest));
  await putMmd(aliceApi, sub.id, text(w.sub));
  await putMmd(aliceApi, hidden.id, "Rien à chercher ici.\n");
  await putMmd(aliceApi, deep.id, text(w.deep));
  await putMmd(aliceApi, trashed.id, text(w.trash));
  // Les droits sont posés avant d'ouvrir les pages.
  const guest = await guestOf(alice, invited.id, `Invité ${tag}`, "read");
  const guestApi = await apiAs(guest);
  const { token } = await inviteToPage(aliceApi, deep.id, { email: guest.email, level: "read" });
  expect((await acceptInvitation(guestApi, token)).changed).toBe(true);
  for (const [own, page] of [
    [w.open, open],
    [w.priv, priv],
    [w.guest, invited],
    [w.sub, sub],
    [w.deep, deep],
    [w.trash, trashed],
  ] as const) {
    await waitFindable(aliceApi, space.id, own, page.id);
  }
  expect((await aliceApi.post(`/api/v1/pages/${trashed.id}/trash`)).status()).toBe(200);

  // Par la route. Témoin : la propriétaire trouve tout, sauf la corbeille.
  expect(await found(aliceApi, space.id, all)).toEqual(sorted([open.id, priv.id, invited.id, sub.id, deep.id]));
  // Le membre : tout l'espace, pas la page privée d'une autre, pas la corbeille.
  expect(await found(brunoApi, space.id, all)).toEqual(sorted([open.id, invited.id, sub.id, deep.id]));
  expect(await found(brunoApi, space.id, w.priv)).toEqual([]);
  // L'invité : ses pages, et rien d'autre.
  expect(await found(guestApi, space.id, all)).toEqual(sorted([invited.id, sub.id, deep.id]));
  await waitIndexed(guestApi, space.id);
  const nobody = JSON.stringify(await searchIn(guestApi, space.id, word()));
  for (const unseen of [w.open, w.priv, w.trash, hiddenTitle]) {
    // Un mot que seule une page non vue porte répond comme un mot que personne n'a écrit.
    expect(JSON.stringify(await searchIn(guestApi, space.id, unseen)), unseen).toBe(nobody);
  }

  // À l'écran de l'invité.
  const p = await openPage(browser, invited.id, guest);
  await openWindow(p);
  const everything = await ask(p, all);
  expect(sorted(everything.pages)).toEqual(sorted([invited.id, sub.id, deep.id]));
  await expect(row(p, deep.id)).toBeVisible();
  expect(sorted(await shown(p))).toEqual(sorted([invited.id, sub.id, deep.id].map((id) => `server:${id}`)));
  // Rien des pages non vues dans la réponse : ni identifiant, ni titre, ni mot, ni fil d'Ariane.
  for (const leak of [open.id, priv.id, trashed.id, hidden.id, hiddenTitle, w.open, w.priv, w.trash, "Privée", "Jetée", "Ouverte"]) {
    expect(everything.text, leak).not.toContain(leak);
  }
  // Le fil d'Ariane de la sous-page vue nomme sa page parente vue ; celui de la page profonde ne
  // nomme pas sa page parente, que l'invité ne voit pas.
  await expect(row(p, sub.id).locator(".mm-search-ancestor")).toHaveText([invited.title]);
  await expect(row(p, deep.id).locator(".mm-search-ancestor")).toHaveCount(0);
  await expect(win(p)).not.toContainText(hiddenTitle);
  await waitIndexed(guestApi, space.id);
  for (const unseen of [w.open, w.priv, w.trash, hiddenTitle]) {
    const answer = await ask(p, unseen);
    expect(answer.text, unseen).toBe(nobody);
    await expect(nothingFound(p)).toBeVisible();
    expect(await shown(p), unseen).toEqual([]);
  }
  // Témoin : la même fenêtre trouve ce que l'invité lit.
  expect((await ask(p, w.deep)).pages).toEqual([deep.id]);
  await expect(row(p, deep.id)).toHaveAttribute("data-from", "server");
  await expect(row(p, deep.id).locator(".mm-search-excerpt")).toContainText(w.deep);

  // Témoin du fil d'Ariane : la propriétaire lit le titre de la page parente dans le sien.
  const owner = await openPage(browser, open.id, alice);
  await openWindow(owner);
  expect((await ask(owner, w.deep)).pages).toEqual([deep.id]);
  await expect(row(owner, deep.id).locator(".mm-search-ancestor")).toHaveText([hidden.title]);
});

test("a_private_page_is_found_by_its_owner_only", async ({ browser }) => {
  const tag = unique();
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const secret = word();
  const titled = word();
  // Le texte porte `secret`, le titre `titled` : ni l'un ni l'autre ne se trouve pour un autre.
  const priv = await createPage(aliceApi, { title: `Journal ${titled}`, section: "private" });
  await putMmd(aliceApi, priv.id, `Le ${secret} ne regarde que moi.\n`);
  const home = await createPage(brunoApi, { title: `Accueil ${tag}`, section: "private" });
  await waitFindable(aliceApi, space.id, secret, priv.id);
  await waitFindable(aliceApi, space.id, titled, priv.id);

  const p = await openPage(browser, home.id, bruno);
  await openWindow(p);
  await waitIndexed(brunoApi, space.id);
  const nobody = (await ask(p, word())).text;
  for (const query of [secret, titled]) {
    const answer = await ask(p, query);
    expect(answer.text, query).toBe(nobody);
    await expect(nothingFound(p)).toBeVisible();
    // Ni ligne du serveur, ni ligne de l'arbre : l'arbre de l'onglet ne porte pas la page.
    expect(await shown(p), query).toEqual([]);
  }

  // Témoin : sa propriétaire la trouve par son texte (le serveur) et par son titre (l'arbre).
  const owner = await openPage(browser, priv.id, alice);
  await openWindow(owner);
  expect((await ask(owner, secret)).pages).toEqual([priv.id]);
  await expect(row(owner, priv.id)).toHaveAttribute("data-from", "server");
  await ask(owner, titled);
  await expect(row(owner, priv.id)).toHaveAttribute("data-from", "tree");
});

test("access_withdrawn_while_the_window_is_open", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const secret = word();
  const shared = await createPage(aliceApi, { title: `Partagée ${tag}`, section: "private" });
  await putMmd(aliceApi, shared.id, `Le ${secret} est partagé un temps.\n`);
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  const home = await createPage(brunoApi, { title: `Accueil ${tag}`, section: "private" });
  await putMmd(brunoApi, home.id, "Chez moi\n");
  await waitFindable(brunoApi, space.id, secret, shared.id);

  const p = await newUser(browser, bruno);
  await p.goto(`/p/${home.id}`);
  await waitSynced(p);
  await openWindow(p);
  expect((await ask(p, secret)).pages).toEqual([shared.id]);
  await expect(row(p, shared.id)).toHaveAttribute("data-from", "server");

  // L'accès est retiré, la fenêtre ouverte. Barrière : le serveur ne montre plus la page à Bruno.
  expect((await aliceApi.delete(`/api/v1/pages/${shared.id}/access/users/${brunoId}`)).status()).toBe(200);
  expect((await brunoApi.get(`/api/v1/pages/${shared.id}`)).status()).toBe(404);
  // La ligne reste tant que ce qui est tapé ne change pas (R1081) ; la recherche suivante ne la
  // rend plus, et le dit comme pour un mot que personne n'a écrit.
  await expect(row(p, shared.id)).toBeVisible();
  await waitIndexed(brunoApi, space.id);
  const nobody = (await ask(p, word())).text;
  await expect(nothingFound(p)).toBeVisible();
  const again = await ask(p, secret);
  expect(again.text).toBe(nobody);
  await expect(nothingFound(p)).toBeVisible();
  expect(await shown(p)).toEqual([]);
});

test("a_line_whose_access_was_just_withdrawn_opens_on_a_refusal", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const secret = word();
  const shared = await createPage(aliceApi, { title: `Partagée ${tag}`, section: "private" });
  await putMmd(aliceApi, shared.id, `Le ${secret} est partagé un temps.\n`);
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  const home = await createPage(brunoApi, { title: `Accueil ${tag}`, section: "private" });
  await putMmd(brunoApi, home.id, "Chez moi\n");
  await waitFindable(brunoApi, space.id, secret, shared.id);

  const p = await newUser(browser, bruno);
  await p.goto(`/p/${home.id}`);
  await waitSynced(p);
  await openWindow(p);
  expect((await ask(p, secret)).pages).toEqual([shared.id]);
  expect((await aliceApi.delete(`/api/v1/pages/${shared.id}/access/users/${brunoId}`)).status()).toBe(200);
  expect((await brunoApi.get(`/api/v1/pages/${shared.id}`)).status()).toBe(404);

  // La ligne est restée : la suivre mène à un refus, dit par la page, sans rien de son contenu.
  const read = p.waitForResponse(
    (res) => new URL(res.url()).pathname === `/api/v1/pages/${shared.id}` && res.request().method() === "GET",
  );
  await row(p, shared.id).click();
  expect((await read).status()).toBe(404);
  await expect(win(p)).toHaveCount(0);
  await expect(accessLost(p)).toBeVisible();
  await expect(p.locator("main")).not.toContainText(secret);
  await expect(p.locator("main")).not.toContainText(shared.title);
  await expect(p.locator(".mm-editor")).toHaveCount(0);
  expect(await focusOnBody(p)).toBe(false);
});

test("a_guest_who_loses_his_only_access_finds_the_focus_on_the_screen_that_says_it", async ({ browser }) => {
  // Recette B, E5 : un invité dont l'unique accès est retiré, la fenêtre ouverte, sort de l'espace.
  // Rien n'est caché : l'onglet l'apprend par la synchro de la page, qui relit la page, puis par
  // l'arbre et le compte. La fenêtre se ferme, l'écran dit l'accès perdu, et le focus, que ni la
  // fenêtre ni l'entrée « Rechercher » (partie avec l'espace) ne peuvent garder, va à son titre.
  const tag = unique();
  const alice = await signUpAccount(`Alice ${tag}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const secret = word();
  const shared = await createPage(aliceApi, { title: `Pour l'invité ${tag}` });
  await putMmd(aliceApi, shared.id, `Le ${secret} est ici.\n`);
  const guest = await guestOf(alice, shared.id, `Invité ${tag}`, "read");
  const guestApi = await apiAs(guest);
  const guestId = (await getMe(guestApi)).user.id;
  await waitFindable(guestApi, space.id, secret, shared.id);

  const p = await openPage(browser, shared.id, guest);
  await openWindow(p);
  expect((await ask(p, secret)).pages).toEqual([shared.id]);
  expect((await aliceApi.delete(`/api/v1/pages/${shared.id}/access/users/${guestId}`)).status()).toBe(200);
  expect((await guestApi.get(`/api/v1/pages/${shared.id}`)).status()).toBe(404);

  await expect(accessLost(p)).toBeVisible({ timeout: 15_000 });
  // Il n'a plus d'espace : l'entrée « Rechercher » part, et la fenêtre avec elle.
  await expect(entry(p)).toHaveCount(0, { timeout: 15_000 });
  await expect(win(p)).toHaveCount(0);
  await expect(accessLost(p)).toBeFocused();
  expect(await focusOnBody(p)).toBe(false);
  await expect(p.locator("main")).not.toContainText(secret);
});

test("a_member_removed_from_the_space_gets_the_answer_of_an_unknown_space", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const [first, second] = [word(), word()];
  const one = await createPage(aliceApi, { title: `Une ${tag}` });
  const two = await createPage(aliceApi, { title: `Deux ${tag}` });
  await putMmd(aliceApi, one.id, `Le ${first} de l'espace.\n`);
  await putMmd(aliceApi, two.id, `Le ${second} de l'espace.\n`);
  await waitFindable(brunoApi, space.id, first, one.id);
  await waitFindable(brunoApi, space.id, second, two.id);

  // L'onglet de Bruno n'apprend pas son retrait par le canal de l'espace ni par la synchro : la
  // fenêtre reste ouverte, et c'est la route qui est jugée.
  const p = await newUser(browser, bruno);
  await closeEventsChannel(p);
  await hideSyncClosures(p);
  await p.goto(`/p/${one.id}`);
  await waitSynced(p);
  await openWindow(p);
  // Témoin : membre, il trouve.
  expect((await ask(p, first)).pages).toEqual([one.id]);

  expect((await aliceApi.delete(`/api/v1/spaces/${space.id}/members/${brunoId}`)).status()).toBe(204);
  const unknown = await brunoApi.post(`/api/v1/spaces/${UNKNOWN_ID}/search`, { data: { q: second } });
  expect(unknown.status()).toBe(404);
  // La recherche suivante reçoit la réponse d'un espace inconnu, octet pour octet, et rien de
  // l'espace ne paraît.
  const answer = await ask(p, second);
  expect(answer.status).toBe(404);
  expect(answer.text).toBe(await unknown.text());
  await expect(row(p, two.id)).toHaveCount(0);
  await expect(p.getByText(second)).toHaveCount(0);
});

test("a_trashed_page_leaves_the_results_and_comes_back", async ({ browser }) => {
  const tag = unique();
  const account = await signUpAccount(`Alice ${tag}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const [own, under] = [word(), word()];
  const thrown = await createPage(api, { title: `Jetable ${tag}` });
  const child = await createPage(api, { title: `Dessous ${tag}`, parentId: thrown.id });
  const home = await createPage(api, { title: `Accueil ${tag}` });
  await putMmd(api, thrown.id, `Le ${own} part à la corbeille.\n`);
  await putMmd(api, child.id, `Le ${under} part avec sa page parente.\n`);
  await putMmd(api, home.id, "Chez moi\n");
  await waitFindable(api, space.id, own, thrown.id);
  await waitFindable(api, space.id, under, child.id);

  const p = await openPage(browser, home.id, account);
  await openWindow(p);
  // Témoin : trouvées avant.
  expect((await ask(p, own)).pages).toEqual([thrown.id]);
  expect((await ask(p, under)).pages).toEqual([child.id]);

  expect((await api.post(`/api/v1/pages/${thrown.id}/trash`)).status()).toBe(200);
  await waitIndexed(api, space.id);
  const nobody = (await ask(p, word())).text;
  for (const query of [own, under]) {
    expect((await ask(p, query)).text, query).toBe(nobody);
    await expect(nothingFound(p)).toBeVisible();
    expect(await shown(p), query).toEqual([]);
  }

  // Restaurée, elle revient, avec sa sous-page, sans attendre l'index (il l'a gardée).
  expect((await api.post(`/api/v1/pages/${thrown.id}/restore`)).status()).toBe(200);
  await ask(p, word());
  expect((await ask(p, own)).pages).toEqual([thrown.id]);
  await expect(row(p, thrown.id)).toHaveAttribute("data-from", "server");
  expect((await ask(p, under)).pages).toEqual([child.id]);
  await expect(row(p, child.id)).toHaveAttribute("data-from", "server");
});

test("another_account_logging_in_in_another_tab_gets_no_result_of_the_first", async ({ browser }) => {
  const tag = unique();
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const [first, second] = [word(), word()];
  const one = await createPage(aliceApi, { title: `Journal un ${tag}`, section: "private" });
  const two = await createPage(aliceApi, { title: `Journal deux ${tag}`, section: "private" });
  // La page ouverte est de l'espace : Bruno la lit aussi, l'onglet rechargé y reste.
  const home = await createPage(aliceApi, { title: `Accueil ${tag}` });
  await putMmd(aliceApi, one.id, `Le ${first} est à Alice.\n`);
  await putMmd(aliceApi, two.id, `Le ${second} est à Alice aussi.\n`);
  await putMmd(aliceApi, home.id, "Pour tous\n");
  await waitFindable(aliceApi, space.id, first, one.id);
  await waitFindable(aliceApi, space.id, second, two.id);

  const p = await openPage(browser, home.id, alice);
  await openWindow(p);
  // Témoin : Alice trouve sa page.
  expect((await ask(p, first)).pages).toEqual([one.id]);
  await expect(row(p, one.id)).toBeVisible();

  // Un autre onglet du navigateur se connecte sous Bruno : le cookie est commun (R540).
  const login = await p.context().request.post("/api/v1/auth/login", {
    data: { email: bruno.email, password: bruno.password },
  });
  expect(login.status(), await login.text()).toBe(200);

  // La recherche suivante de l'onglet d'Alice part sous le cookie de Bruno, au nom d'Alice : le
  // serveur la refuse sans rien chercher, et l'onglet repart sous Bruno.
  const answer = await ask(p, second);
  expect(answer.status).toBe(409);
  expect(answer.text).toContain("account_changed");
  expect(answer.text).not.toContain(two.id);
  await expectSignedInAs(p, bruno.name);
  await expect(win(p)).toHaveCount(0);
  await expect(p.getByText(first)).toHaveCount(0);
  // Sous Bruno, ni l'une ni l'autre ne se trouve.
  await waitSynced(p);
  await openWindow(p);
  await waitIndexed(brunoApi, space.id);
  const nobody = (await ask(p, word())).text;
  for (const query of [first, second]) {
    expect((await ask(p, query)).text, query).toBe(nobody);
    await expect(nothingFound(p)).toBeVisible();
    expect(await shown(p), query).toEqual([]);
  }
});

test("erased_text_is_no_longer_found", async ({ browser }) => {
  const tag = unique();
  const account = await signUpAccount(`Alice ${tag}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const [erased, kept] = [word(), word()];
  const page = await createPage(api, { title: `Brouillon ${tag}` });
  await putMmd(api, page.id, `Le ${erased} va partir.\n\nLe ${kept} reste.\n`);
  await waitFindable(api, space.id, erased, page.id);

  const p = await openPage(browser, page.id, account);
  // Le paragraphe est remplacé dans l'éditeur (un triple clic le sélectionne en entier).
  await p.locator(".mm-editor .bn-block-content").first().click({ clickCount: 3 });
  await p.keyboard.type("Autre chose.");
  await expect.poll(() => getMmd(api, page.id)).toBe(`Autre chose.\n\nLe ${kept} reste.\n`);
  // Limite L-a : le texte effacé reste trouvable le temps de l'indexation. Une condition, pas une
  // durée : la route ne le rend plus, et ne dit plus l'index en retard.
  await expect
    .poll(
      async () => {
        const answer = await searchIn(api, space.id, erased);
        return { pages: answer.results.length, indexing: answer.indexing };
      },
      { timeout: 30_000 },
    )
    .toEqual({ pages: 0, indexing: false });

  await openWindow(p);
  await waitIndexed(api, space.id);
  const nobody = (await ask(p, word())).text;
  const answer = await ask(p, erased);
  expect(answer.text).toBe(nobody);
  await expect(nothingFound(p)).toBeVisible();
  expect(await shown(p)).toEqual([]);
  // Témoin : le texte resté se trouve toujours, dans la même page.
  expect((await ask(p, kept)).pages).toEqual([page.id]);
  await expect(row(p, page.id).locator(".mm-search-excerpt")).toContainText(kept);
});

test("a_role_in_two_spaces_never_mixes_their_pages", async ({ browser }) => {
  const tag = unique();
  const alice = await signUpAccount(`Alice ${tag}`);
  const aliceApi = await apiAs(alice);
  const here = await firstSpace(aliceApi);
  const bruno = await signUpAccount(`Bruno ${tag}`);
  const brunoApi = await apiAs(bruno);
  const there = await firstSpace(brunoApi);
  await joinSpace(brunoApi, there.id, alice, "member");
  // Le même mot dans une page de chaque espace, qu'Alice lit toutes deux, et dans une page privée
  // de Bruno, qu'elle ne lit pas.
  const common = word();
  const mine = await createPage(aliceApi, { spaceId: here.id, title: `Ici ${tag}` });
  const theirs = await createPage(brunoApi, { spaceId: there.id, title: `Là-bas ${tag}` });
  const closed = await createPage(brunoApi, { spaceId: there.id, title: `Fermée ${tag}`, section: "private" });
  await putMmd(aliceApi, mine.id, `Le ${common} de chez moi.\n`);
  await putMmd(brunoApi, theirs.id, `Le ${common} de chez Bruno.\n`);
  await putMmd(brunoApi, closed.id, `Le ${common} de Bruno seul.\n`);
  await waitFindable(aliceApi, here.id, common, mine.id);
  await waitFindable(brunoApi, there.id, common, theirs.id);
  await waitFindable(brunoApi, there.id, common, closed.id);

  // Par la route : chaque espace ne rend que ses pages.
  expect(await found(aliceApi, here.id, common)).toEqual([mine.id]);
  expect(await found(aliceApi, there.id, common)).toEqual([theirs.id]);

  // À l'écran : l'espace affiché est celui de la page ouverte.
  const p = await openPage(browser, mine.id, alice);
  await expect(spaceSwitcher(p, here.name)).toBeVisible();
  await openWindow(p);
  const inHere = await ask(p, common);
  expect(inHere.pages).toEqual([mine.id]);
  for (const leak of [theirs.id, closed.id, "chez Bruno", "Bruno seul", "Là-bas", "Fermée"]) {
    expect(inHere.text, leak).not.toContain(leak);
  }
  await expect(row(p, mine.id)).toBeVisible();
  expect(await shown(p)).toEqual([`server:${mine.id}`]);

  await p.goto(`/p/${theirs.id}`);
  await waitSynced(p);
  await expect(spaceSwitcher(p, there.name)).toBeVisible();
  await openWindow(p);
  const inThere = await ask(p, common);
  expect(inThere.pages).toEqual([theirs.id]);
  for (const leak of [mine.id, closed.id, "chez moi", "Bruno seul", "Ici", "Fermée"]) {
    expect(inThere.text, leak).not.toContain(leak);
  }
  await expect(row(p, theirs.id)).toBeVisible();
  expect(await shown(p)).toEqual([`server:${theirs.id}`]);
});

// Les gestes de l'arrivée sur un bloc (Task C2), pour qui ne fait que lire.

const blockUrl = (id: string) => new RegExp(`/p/${id}#b=.+$`);
const blockById = (p: Page, blockId: string) =>
  p.locator(".mm-editor [data-node-type='blockOuter']").and(p.locator(`[data-id="${blockId}"]`));
/** Où sont le focus et la sélection du document : dans l'éditeur ? dans quel bloc ? */
const cursorAt = (p: Page) =>
  p.evaluate(() => {
    const node = document.getSelection()?.anchorNode ?? null;
    const element = node instanceof Element ? node : (node?.parentElement ?? null);
    return {
      inEditor: document.activeElement?.closest(".mm-editor") != null,
      block: element?.closest('[data-node-type="blockOuter"]')?.getAttribute("data-id") ?? null,
    };
  });
/** Attend que l'adresse soit celle de la page `id` à un bloc ; rend l'identifiant du bloc, décodé. */
async function blockOfUrl(p: Page, id: string): Promise<string> {
  await expect(p).toHaveURL(blockUrl(id));
  return decodeURIComponent(new URL(p.url()).hash.slice("#b=".length));
}
const filler = (from: number, n: number) => Array.from({ length: n }, (_, i) => `Ligne ${from + i} du remplissage.`);

test("a_reader_of_a_living_page_arrives_on_the_found_block", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const w = { plain: word(), column: word(), cell: word(), folded: word() };
  const long = await createPage(aliceApi, { title: `Longue ${tag}`, section: "private" });
  const mmd = [
    ...filler(0, 60),
    `Le ${w.plain} est un paragraphe.`,
    ...filler(60, 60),
    `<columns>\n<column>\nLe ${w.column} est dans une colonne.\n</column>\n<column>\nL'autre colonne.\n</column>\n</columns>`,
    ...filler(120, 60),
    `| Nom | Statut |\n| --- | --- |\n| ${w.cell} | Fini |`,
    ...filler(180, 60),
    `<toggle title="Un repli">\nLe ${w.folded} est sous le repli.\n</toggle>`,
    ...filler(240, 20),
  ];
  await putMmd(aliceApi, long.id, `${mmd.join("\n\n")}\n`);
  // Bruno ne fait que lire la page, qui vit (ni corbeille, ni autre schéma).
  await setPageLevel(aliceApi, long.id, brunoId, "read");
  const home = await createPage(brunoApi, { title: `Accueil ${tag}`, section: "private" });
  await putMmd(brunoApi, home.id, "Chez moi\n");
  for (const own of Object.values(w)) await waitFindable(brunoApi, space.id, own, long.id);

  for (const [what, own] of Object.entries(w)) {
    // Un onglet neuf par bloc : chaque arrivée part d'une autre page.
    const p = await openPage(browser, home.id, bruno);
    await openWindow(p);
    expect((await ask(p, own)).pages, what).toEqual([long.id]);
    await expect(row(p, long.id)).toHaveAttribute("aria-selected", "true");
    await p.keyboard.press("Enter");
    const blockId = await blockOfUrl(p, long.id);
    await waitSynced(p);
    await expect(win(p)).toHaveCount(0);
    await expect(p.locator(".mm-editor")).toHaveAttribute("data-editable", "false");
    await expect(readerBanner(p)).toBeVisible();
    await expect(p.locator('[role="alert"]')).toHaveCount(0);
    expect(await focusOnBody(p), what).toBe(false);
    const block = blockById(p, blockId);
    await expect(block, what).toHaveCount(1);
    if (what === "folded") {
      // Limite écrite : un repli fermé n'est pas ouvert. Le bloc existe, la page s'ouvre sans rien
      // dire, et le focus n'est pas perdu.
      continue;
    }
    await expect(block, what).toContainText(own);
    await expect(block, what).toBeInViewport();
    await expect(p.locator(".mm-editor [data-node-type='blockOuter']").first(), what).not.toBeInViewport();
    await expect.poll(() => cursorAt(p), what).toEqual({ inEditor: true, block: blockId });
    // Lecture seule : une frappe n'écrit rien.
    await p.keyboard.type("x");
    await expect(block, what).not.toContainText(`x${own}`);
  }
  expect(await getMmd(aliceApi, long.id)).toBe(`${mmd.join("\n\n")}\n`);
});

/** Une longue page d'un compte neuf, dont un paragraphe loin en bas porte un mot ; son bloc. */
async function farBlock() {
  const tag = unique();
  const account = await signUpAccount(`Alice ${tag}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const far = word();
  const long = await createPage(api, { title: `Longue ${tag}` });
  await putMmd(api, long.id, `${[...filler(0, 150), `Le ${far} est loin en bas.`, ...filler(150, 20)].join("\n\n")}\n`);
  await waitFindable(api, space.id, far, long.id);
  const hit = (await searchIn(api, space.id, far)).results[0];
  expect(hit.blockId).not.toBeNull();
  return { account, api, long, far, blockId: hit.blockId! };
}

test("a_cold_arrival_shows_the_block_and_leaves_a_keystroke_to_the_title", async ({ browser }) => {
  const { account, api, long, far, blockId } = await farBlock();
  const p = await newUser(browser, account);
  // La seconde lecture de la page (celle d'avant la connexion de synchro) est retenue : la page est
  // ouverte, son titre se tape, la première synchro n'est pas faite.
  let release = () => {};
  const released = new Promise<void>((resolve) => (release = resolve));
  let held = () => {};
  const gateAsked = new Promise<void>((resolve) => (held = resolve));
  let pageReads = 0;
  await p.route(`**/api/v1/pages/${long.id}`, async (route) => {
    if (route.request().method() === "GET" && ++pageReads > 1) {
      held();
      await released;
    }
    await route.continue();
  });
  await p.goto(`/p/${long.id}#b=${encodeURIComponent(blockId)}`);
  await gateAsked;
  await expect(titleOf(p)).toHaveValue(long.title);
  await titleOf(p).click();
  await p.keyboard.press("End");
  await p.keyboard.type(" tap");
  release();
  await waitSynced(p);
  // Le bloc visé est montré…
  const block = blockById(p, blockId);
  await expect(block).toContainText(far);
  await expect(block).toBeInViewport();
  // … et la frappe reste au titre : le focus n'a pas été pris.
  await expect(titleOf(p)).toBeFocused();
  await p.keyboard.type("é");
  await expect(titleOf(p)).toHaveValue(`${long.title} tapé`);
  await expect(block).toHaveText(`Le ${far} est loin en bas.`);
  await p.keyboard.press("Enter");
  await expect.poll(async () => (await api.get(`/api/v1/pages/${long.id}`)).json()).toMatchObject({
    title: `${long.title} tapé`,
  });
});

test("an_anonymous_on_a_block_address_reaches_the_block_once_logged_in", async ({ browser }) => {
  const { account, long, far, blockId } = await farBlock();
  const context = await browser.newContext({ storageState: NO_SESSION });
  try {
    const p = await context.newPage();
    await p.goto(`/p/${long.id}#b=${encodeURIComponent(blockId)}`);
    await logInWith(p, account.email, account.password);
    await waitSynced(p);
    await expect(titleOf(p)).toHaveValue(long.title);
    // Le fragment est gardé jusqu'à la page : le bloc visé est à l'écran.
    expect(await blockOfUrl(p, long.id)).toBe(blockId);
    const block = blockById(p, blockId);
    await expect(block).toContainText(far);
    await expect(block).toBeInViewport();
    expect(await focusOnBody(p)).toBe(false);
  } finally {
    await context.close();
  }
});

// Ctrl+K pendant qu'un menu de BlockNote est ouvert (« / », `@`, `[[`).

for (const [name, typed, menu] of [
  ["slash", "/tit", (p: Page) => p.locator(".bn-suggestion-menu")],
  ["at", "@Acc", (p: Page) => p.getByRole("listbox", { name: "Mentionner" })],
  ["brackets", "[[Acc", (p: Page) => p.locator("#bn-suggestion-menu")],
] as const) {
  test(`the_shortcut_while_the_${name}_menu_is_open_opens_the_search_and_loses_nothing`, async ({ browser }) => {
    const tag = unique();
    const account = await signUpAccount(`Alice ${tag}`);
    const api = await apiAs(account);
    const home = await createPage(api, { title: `Accueil ${tag}` });
    await putMmd(api, home.id, "Début\n");
    const p = await openPage(browser, home.id, account);
    const block = p.locator(".mm-editor .bn-block-content").first();
    await block.click();
    await p.keyboard.press("End");
    await p.keyboard.type(` ${typed}`);
    await expect(menu(p)).toBeVisible();

    await p.keyboard.press("Control+k");
    // La fenêtre de recherche s'ouvre, son champ a le focus, et ce qu'on y tape y va.
    await expect(field(p)).toBeFocused();
    await p.keyboard.type("abc");
    await expect(field(p)).toHaveValue("abc");
    // Rien de tapé n'est perdu ni changé dans la page.
    await expect(block).toHaveText(`Début ${typed}`);
    await p.keyboard.press("Escape");
    await expect(win(p)).toHaveCount(0);
    // Le menu de BlockNote s'est fermé avec le focus parti (il ne se rouvre pas), et le focus
    // revient au texte, où la frappe reprend, à sa place.
    await expect(menu(p)).toHaveCount(0);
    expect(await p.evaluate(() => document.activeElement?.closest(".mm-editor") != null)).toBe(true);
    await p.keyboard.type("!");
    await expect(block).toHaveText(`Début ${typed}!`);
    // Le MMD échappe les crochets : comparé sans ses barres obliques inverses.
    await expect
      .poll(async () => (await getMmd(api, home.id)).replaceAll("\\", ""))
      .toBe(`Début ${typed}!\n`);
    // Aucune page créée, aucune mention posée par le menu qui s'est fermé.
    await expect(p.locator(".mm-editor .mm-mention")).toHaveCount(0);
  });
}

// Échap dans le texte de l'éditeur, sans menu ouvert, ne fait rien (question 37 de la recette du lot
// 4, choix (a), R1143) : le curseur reste où il est, et la frappe suivante s'y écrit. BlockNote 0.57
// retirait le focus de l'éditeur (son extension `OverrideEscape`), et le laissait sur `body`. Avec du
// texte sélectionné, Échap ferme la barre de mise en forme, comme avant, sans perdre le focus : la
// sélection se réduit à sa fin.
test("escape_in_the_text_of_the_editor_keeps_the_focus_off_body", async ({ browser }) => {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const home = await createPage(api, { title: `Accueil ${unique()}` });
  await putMmd(api, home.id, "Début\n");
  const p = await openPage(browser, home.id, account);
  const block = p.locator(".mm-editor .bn-block-content").first();
  await block.click();
  await p.keyboard.press("End");
  await p.keyboard.press("Escape");
  expect(await focusOnBody(p)).toBe(false);
  await p.keyboard.type("!");
  await expect(block).toHaveText("Début!");

  // « but » sélectionné : la barre de mise en forme paraît ; Échap la ferme, le curseur à la fin de
  // la sélection, et rien n'est remplacé.
  await expect(async () => {
    await p.evaluate(() => {
      const selection = document.getSelection()!;
      if (String(selection) === "but") return;
      const first = document.querySelector(".mm-editor .bn-block-content")!;
      const text = document.createTreeWalker(first, NodeFilter.SHOW_TEXT).nextNode()!;
      selection.setBaseAndExtent(text, "Dé".length, text, "Début".length);
    });
    await expect(p.getByRole("button", { name: "Créer un lien" })).toBeVisible({ timeout: 2_000 });
  }).toPass({ timeout: 20_000 });
  await p.keyboard.press("Escape");
  await expect(p.getByRole("button", { name: "Créer un lien" })).toHaveCount(0);
  expect(await focusOnBody(p)).toBe(false);
  await p.keyboard.type("?");
  await expect(block).toHaveText("Début?!");
  await expect.poll(() => getMmd(api, home.id)).toBe("Début?!\n");
});
