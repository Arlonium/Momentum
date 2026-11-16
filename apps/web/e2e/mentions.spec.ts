// Les mentions (Task C4, R251) : `@` pose une page, une personne ou une date ; `[[` lie une page, ou
// crée une sous-page et la lie. La mention porte le libellé copié ; le serveur la relit en MMD
// (`<mention type="page" id="…">Plan</mention>`). Aucun menu ne propose à quelqu'un une page qu'il
// ne voit pas, et rien de ce que reçoit son onglet n'en porte le titre (preuve 4 du plan).
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  getMmd,
  getTree,
  guestOf,
  newUser,
  openPage,
  ownerAndMember,
  signUpAccount,
  titleOf,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

/**
 * Mona (propriétaire de l'espace) et Tom (membre), une page de « Espace », vide, que les deux ont
 * ouverte, et, créée avant, la page de « Espace » au titre `other` s'il est donné : elle est dans
 * l'arbre de chacun quand il ouvre le menu (un menu ouvert ne suit pas l'arbre entre deux frappes).
 */
async function duo(browser: Parameters<typeof openPage>[0], other?: string) {
  const team = await ownerAndMember();
  const linked = other === undefined ? null : await createPage(team.aliceApi, { spaceId: team.space.id, title: other });
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Mentions ${unique()}` });
  const mona = await openPage(browser, page.id, team.alice);
  const tom = await openPage(browser, page.id, team.bruno);
  return { ...team, page, linked, mona, tom };
}

/** Met le curseur dans le premier bloc de l'éditeur, et tape `text`. */
async function typeInFirstBlock(p: Page, text: string): Promise<void> {
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.type(text);
}

const atMenu = (p: Page) => p.getByRole("listbox", { name: "Mentionner" });
const bracketMenu = (p: Page) => p.getByRole("listbox", { name: "Lier une page" });
const mentionsOf = (p: Page) => p.locator(".mm-editor .mm-mention");

/** Les lignes du groupe `name` d'un menu. */
const group = (menu: ReturnType<typeof atMenu>, name: string) => menu.getByRole("group", { name }).getByRole("option");

test("page_mention_is_typed_seen_and_read_back", async ({ browser }) => {
  const tag = unique();
  const { mona, tom, aliceApi, page, linked } = await duo(browser, `Plan ${tag}`);
  const plan = linked!;

  await typeInFirstBlock(mona, `Voir @Plan ${tag}`);
  await expect(group(atMenu(mona), "Pages")).toHaveText([`Plan ${tag}`]);
  // Rien n'est une mention tant qu'on cherche : le texte tapé est du texte.
  await expect(mentionsOf(tom)).toHaveCount(0);
  await mona.keyboard.press("Enter");
  await expect(atMenu(mona)).toBeHidden();
  // La mention est posée, suivie d'une espace : la frappe reprend après elle.
  await expect(mentionsOf(mona)).toHaveText([`@Plan ${tag}`]);
  await mona.keyboard.type("demain");

  // Tom la voit ; le serveur la relit en MMD, avec l'id de la page et le titre copié.
  await expect(mentionsOf(tom)).toHaveText([`@Plan ${tag}`]);
  await expect
    .poll(() => getMmd(aliceApi, page.id))
    .toBe(`Voir <mention type="page" id="${plan.id}">Plan ${tag}</mention> demain\n`);

  // La page renommée : la mention suit chez qui la voit ; le document garde le titre copié.
  const renamed = await aliceApi.patch(`/api/v1/pages/${plan.id}`, { data: { title: `Plan 2027 ${tag}` } });
  expect(renamed.status(), await renamed.text()).toBe(200);
  await expect(mentionsOf(tom)).toHaveText([`@Plan 2027 ${tag}`]);
  await expect(mentionsOf(mona)).toHaveText([`@Plan 2027 ${tag}`]);
  expect(await getMmd(aliceApi, page.id)).toBe(`Voir <mention type="page" id="${plan.id}">Plan ${tag}</mention> demain\n`);

  // Un clic sur la mention ouvre la page, sans recharger l'application, dans un éditeur modifiable.
  await tom.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  await mentionsOf(tom).click();
  await expect(tom).toHaveURL(new RegExp(`/p/${plan.id}$`));
  await expect(titleOf(tom)).toHaveValue(`Plan 2027 ${tag}`);
  expect(await tom.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
});

test("person_and_date_mentions_read_back", async ({ browser }) => {
  const { mona, tom, aliceApi, bruno, brunoId, page } = await duo(browser);
  /** Les requêtes de l'onglet de Mona vers la liste des membres. */
  const memberReads: string[] = [];
  mona.on("request", (request) => {
    if (request.url().includes("/members")) memberReads.push(request.url());
  });

  // Les personnes ne sont lues qu'à l'ouverture du menu.
  await typeInFirstBlock(mona, "Pour ");
  expect(memberReads).toEqual([]);
  await mona.keyboard.type(`@${bruno.name}`);
  await expect(group(atMenu(mona), "Personnes")).toHaveText([bruno.name]);
  expect(memberReads.length).toBe(1);
  await mona.keyboard.press("Enter");
  await expect(mentionsOf(mona)).toHaveText([`@${bruno.name}`]);

  // Une date, dite en chiffres : le menu la garde tout du long, « / » compris.
  await mona.keyboard.type("le @01/12/2026");
  await expect(group(atMenu(mona), "Dates")).toHaveText(["1 déc. 2026"]);
  await mona.keyboard.press("Enter");
  await mona.keyboard.type("au plus tard");
  await expect(mentionsOf(tom)).toHaveText([`@${bruno.name}`, "@1 déc. 2026"]);
  await expect
    .poll(() => getMmd(aliceApi, page.id))
    .toBe(
      `Pour <mention type="user" id="${brunoId}">${bruno.name}</mention> le ` +
        `<mention type="date" id="2026-12-01">1 déc. 2026</mention> au plus tard\n`,
    );
  // Une seule lecture des membres pour toutes ces frappes.
  expect(memberReads.length).toBe(1);

  // Une date dite en mots ; Échap ferme le menu et laisse le texte tapé.
  await mona.keyboard.type(" @dem");
  await expect(group(atMenu(mona), "Dates")).toContainText(["Demain"]);
  await mona.keyboard.press("Escape");
  await expect(atMenu(mona)).toBeHidden();
  await expect.poll(() => getMmd(aliceApi, page.id)).toContain("au plus tard @dem\n");
  await expect(mentionsOf(mona)).toHaveCount(2);
});

test("double_bracket_creates_a_subpage_and_links_it", async ({ browser }) => {
  const tag = unique();
  const { mona, tom, aliceApi, space, page, linked } = await duo(browser, `Compte ${tag}`);
  const other = linked!;

  // `[[` propose les pages ; « Créer la page » vient en dernier, dès qu'un titre est tapé.
  await typeInFirstBlock(mona, `Lire [[Compte ${tag}`);
  await expect(bracketMenu(mona).getByRole("option")).toHaveText([`Compte ${tag}`, `Créer la page « Compte ${tag} »`]);
  await mona.keyboard.type(" rendu");
  const create = bracketMenu(mona).getByRole("option", { name: `Créer la page « Compte ${tag} rendu »` });
  await expect(bracketMenu(mona).getByRole("option")).toHaveText([`Créer la page « Compte ${tag} rendu »`]);
  await expect(create).toHaveAttribute("aria-selected", "true");
  await mona.keyboard.press("Enter");

  // La sous-page est créée une fois, sous la page ouverte, et mentionnée là où l'on tapait.
  await expect(mentionsOf(mona)).toHaveText([`@Compte ${tag} rendu`]);
  await mona.keyboard.type("ensuite");
  const created = async () => (await getTree(aliceApi, space.id)).nodes.filter((n) => n.title === `Compte ${tag} rendu`);
  await expect.poll(async () => (await created()).length).toBe(1);
  const [child] = await created();
  expect(child.parentId).toBe(page.id);
  await expect
    .poll(() => getMmd(aliceApi, page.id))
    .toBe(`Lire <mention type="page" id="${child.id}">Compte ${tag} rendu</mention> ensuite\n`);
  // Mona n'a pas quitté la page.
  await expect(mona).toHaveURL(new RegExp(`/p/${page.id}$`));

  // Chez Tom : la mention, et la sous-page dans sa barre latérale, sous la page.
  await expect(mentionsOf(tom)).toHaveText([`@Compte ${tag} rendu`]);
  const parentRow = treeRow(tom, page.id);
  await expect(parentRow).toHaveAttribute("aria-expanded", /true|false/);
  if ((await parentRow.getAttribute("aria-expanded")) === "false") {
    await parentRow.getByRole("button", { name: "Déplier" }).click();
  }
  await expect(treeRow(tom, child.id).getByRole("link")).toHaveText(`Compte ${tag} rendu`);

  // `[[` lie aussi une page qui existe : choisie, elle est mentionnée, sans rien créer.
  await mona.keyboard.type(` et [[Compte ${tag}`);
  await expect(bracketMenu(mona).getByRole("option").first()).toHaveText(`Compte ${tag}`);
  await mona.keyboard.press("Enter");
  await expect(mentionsOf(mona)).toHaveText([`@Compte ${tag} rendu`, `@Compte ${tag}`]);
  await expect.poll(() => getMmd(aliceApi, page.id)).toContain(`<mention type="page" id="${other.id}">Compte ${tag}</mention>`);
  expect((await created()).length).toBe(1);
});

test("guest_is_offered_only_the_pages_he_sees", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const tag = unique();
  const secret = `Plan secret ${tag}`;
  await createPage(aliceApi, { title: secret });
  const notes = await createPage(aliceApi, { title: `Notes ${tag}` });
  // Gina n'est invitée que sur « Notes », où elle peut écrire.
  const gina = await guestOf(alice, notes.id, `Gina ${unique()}`, "edit");

  /** Tout ce que reçoit un onglet, et ce qu'il demande. */
  const listen = (p: Page) => {
    const received: string[] = [];
    const asked: string[] = [];
    p.on("request", (request) => asked.push(request.url()));
    p.on("response", (response) => {
      if (!response.url().includes("/api/")) return;
      void response.text().then(
        (text) => received.push(text),
        () => {},
      );
    });
    p.on("websocket", (socket) => {
      socket.on("framereceived", ({ payload }) =>
        received.push(typeof payload === "string" ? payload : payload.toString("utf8")),
      );
    });
    return { received, asked };
  };

  // Témoin : Alice, qui voit les deux pages, se voit proposer la page secrète, par `@` comme par
  // `[[`, et son onglet en a reçu le titre.
  const a = await newUser(browser, alice);
  const toAlice = listen(a);
  await a.goto(`/p/${notes.id}`);
  await waitSynced(a);
  await typeInFirstBlock(a, `@${tag}`);
  await expect(group(atMenu(a), "Pages")).toHaveText([secret, `Notes ${tag}`]);
  await a.keyboard.press("Escape");
  await a.keyboard.type(` [[${tag}`);
  await expect(bracketMenu(a).getByRole("option")).toHaveText([secret, `Notes ${tag}`, `Créer la page « ${tag} »`]);
  await a.keyboard.press("Escape");
  await expect(bracketMenu(a)).toBeHidden();
  await expect.poll(() => toAlice.received.some((text) => text.includes(secret))).toBe(true);
  // Alice, membre, se voit proposer Gina, invitée de l'espace, qui n'a pas encore ouvert la page.
  await a.keyboard.type(` @${gina.name}`);
  await expect(group(atMenu(a), "Personnes")).toHaveText([gina.name]);
  await a.keyboard.press("Escape");

  // Gina tape les mêmes recherches : seule sa page lui est proposée.
  const g = await newUser(browser, gina);
  const toGina = listen(g);
  await g.goto(`/p/${notes.id}`);
  await waitSynced(g);
  await g.locator(".mm-editor .bn-block-content").last().click();
  await g.keyboard.press("End");
  await g.keyboard.type(` @${tag}`);
  await expect(group(atMenu(g), "Pages")).toHaveText([`Notes ${tag}`]);
  await g.keyboard.press("Escape");
  await expect(atMenu(g)).toBeHidden();
  await g.keyboard.type(` [[${tag}`);
  await expect(bracketMenu(g).getByRole("option")).toHaveText([`Notes ${tag}`, `Créer la page « ${tag} »`]);
  await g.keyboard.press("Escape");
  await expect(bracketMenu(g)).toBeHidden();
  // Sans rien taper : les pages qu'elle voit, et elles seules.
  await g.keyboard.type(" [[");
  await expect(bracketMenu(g).getByRole("option")).toHaveText([`Notes ${tag}`]);
  await g.keyboard.press("Escape");
  await expect(bracketMenu(g)).toBeHidden();

  // Les personnes : qui a la page ouverte (Alice et elle), sans que la liste des membres soit demandée.
  await g.keyboard.type(" @");
  await expect(group(atMenu(g), "Personnes")).toHaveText([alice.name, gina.name]);
  await g.keyboard.press("Escape");
  expect(toGina.asked.filter((url) => url.includes("/members"))).toEqual([]);
  // Témoin de cette absence : l'onglet d'Alice, lui, l'a demandée.
  expect(toAlice.asked.filter((url) => url.includes("/members")).length).toBeGreaterThan(0);

  // Rien de ce que l'onglet de Gina a reçu ne porte le titre de la page qu'elle ne voit pas.
  // Témoin de l'écoute : le titre de sa page, lui, est bien passé par là.
  await expect.poll(() => toGina.received.some((text) => text.includes(`Notes ${tag}`))).toBe(true);
  expect(await g.content()).not.toContain(secret);
  expect(toGina.received.filter((text) => text.includes(secret))).toEqual([]);
});

test("at_sign_typed_with_altgr_opens_the_menu", async ({ browser }) => {
  const { mona, aliceApi, page } = await duo(browser);
  await typeInFirstBlock(mona, "Voir ");
  // Un clavier AZERTY : « @ » est AltGr+0. Sous Windows, AltGr arrive comme Ctrl+Alt (modificateurs 1
  // et 2 du protocole de Chromium) ; ailleurs, sans Ctrl ni Alt. Les deux portent la touche « @ » sur
  // la position `Digit0`.
  const cdp = await mona.context().newCDPSession(mona);
  const altGr = process.platform === "win32" ? 3 : 0;
  const key = { key: "@", code: "Digit0", windowsVirtualKeyCode: 48, nativeVirtualKeyCode: 48, modifiers: altGr };
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", ...key, text: "@", unmodifiedText: "à" });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...key });
  await expect(atMenu(mona)).toBeVisible();
  // Le caractère est dans le texte, et la recherche continue.
  await mona.keyboard.type("dem");
  await expect(group(atMenu(mona), "Dates")).toContainText(["Demain"]);
  await mona.keyboard.press("Escape");
  await expect(atMenu(mona)).toBeHidden();
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("Voir @dem\n");
});
