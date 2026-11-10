// Déplacer une page dans la barre latérale : à la souris (glisser-déposer : la cible montrée,
// le trait d'insertion, les dépôts refusés) ou au clavier, par le menu « … » de sa ligne
// (Ruling R95). Le serveur fait foi : chaque test relit aussi l'arbre qu'il rend.
import { expect, test, type APIRequestContext } from "@playwright/test";
import type { ErrorBody } from "@mementum/protocol";
import {
  apiAs,
  byPosition,
  closeContexts,
  createPage,
  firstSpace,
  getMe,
  getTree,
  newUser,
  NO_SESSION,
  openRowMenu,
  ownerAndMember,
  sectionOf,
  setPageLevel,
  shownRows,
  sidebarOf,
  signUpAccount,
  titleOf,
  treeRow,
  unique,
} from "./helpers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** Ce que le compte de `api` voit des pages de l'espace : où est chacune, et dans quelle section. */
async function placesSeenBy(api: APIRequestContext, spaceId: string) {
  const { nodes } = await getTree(api, spaceId);
  return Object.fromEntries(
    nodes.map((n) => [n.title, `${n.parentId === null ? "racine" : "sous-page"}, ${n.section}`]),
  );
}

test("drag_a_page_under_another", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });
  const mine = await createPage(api, { section: "private", title: "Delta" });
  /** L'arbre du serveur : sous chaque page parente (ou à la racine), ses pages dans l'ordre. */
  const serverTree = async () => {
    const { nodes } = await getTree(api, space.id);
    const under: Record<string, string[]> = {};
    for (const node of byPosition(nodes)) {
      const parent = nodes.find((n) => n.id === node.parentId)?.title ?? "racine";
      (under[parent] ??= []).push(node.title);
    }
    return under;
  };

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie", "1 Delta"]);

  // « Charlie », lâchée au milieu de la ligne de « Bravo », devient sa sous-page.
  await treeRow(p, c.id).dragTo(treeRow(p, b.id));
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "2 Charlie", "1 Delta"]);
  await expect(treeRow(p, b.id)).toHaveAttribute("aria-expanded", "true");
  expect((await getTree(api, space.id)).nodes.find((n) => n.id === c.id)?.parentId).toBe(b.id);
  expect(await serverTree()).toEqual({ racine: ["Alpha", "Bravo", "Delta"], Bravo: ["Charlie"] });

  // « Bravo », lâchée sur le bord haut de « Alpha », passe avant elle, avec sa sous-page.
  await treeRow(p, b.id).dragTo(treeRow(p, a.id), { targetPosition: { x: 80, y: 2 } });
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "2 Charlie", "1 Alpha", "1 Delta"]);
  expect(await serverTree()).toEqual({ racine: ["Bravo", "Alpha", "Delta"], Bravo: ["Charlie"] });

  // Refusés, et sans effet : une page sous sa propre sous-page, et d'une section à l'autre.
  const before = await serverTree();
  await treeRow(p, b.id).dragTo(treeRow(p, c.id));
  await treeRow(p, mine.id).dragTo(treeRow(p, a.id));
  await treeRow(p, a.id).dragTo(treeRow(p, mine.id));
  await p.waitForTimeout(300);
  expect(await shownRows(p)).toEqual(["1 Bravo", "2 Charlie", "1 Alpha", "1 Delta"]);
  expect(await serverTree()).toEqual(before);
  await expect(sidebarOf(p).getByRole("alert")).toHaveCount(0);

  // Tout est gardé au rechargement.
  await p.reload();
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "2 Charlie", "1 Alpha", "1 Delta"]);
});

test("refused_move_comes_back_with_the_server_message", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo"]);

  // Pendant que la barre latérale est ouverte, « Bravo » part à la corbeille ailleurs.
  expect((await api.post(`/api/v1/pages/${b.id}/trash`)).status()).toBe(200);
  // Ce que le serveur répond désormais à qui y range une page.
  const refused = await api.patch(`/api/v1/pages/${a.id}`, { data: { parentId: b.id } });
  expect(refused.ok()).toBe(false);
  const body = (await refused.json()) as ErrorBody;

  // Le dépôt est refusé : le message du serveur, la page à sa place, et la barre latérale
  // telle que le serveur la voit.
  await treeRow(p, a.id).dragTo(treeRow(p, b.id));
  await expect(sidebarOf(p).getByRole("alert")).toContainText(body.message, { ignoreCase: true });
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha"]);
  expect((await getTree(api, space.id)).nodes.map((n) => [n.id, n.parentId])).toEqual([[a.id, null]]);
  // Le message part à la navigation suivante.
  await treeRow(p, a.id).getByRole("link").click();
  await expect(titleOf(p)).toHaveValue("Alpha");
  await expect(sidebarOf(p).getByRole("alert")).toHaveCount(0);
});

test("row_menu_by_keyboard", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const button = treeRow(p, b.id).getByRole("button", { name: "Actions de « Bravo »" });
  const menu = p.getByRole("menu", { name: "Actions de « Bravo »" });
  const item = (name: string) => menu.getByRole("menuitem", { name });

  // Ouvert par Entrée : le focus est sur sa première action ; les flèches passent de l'une à
  // l'autre, en boucle ; Fin et Début vont aux extrémités.
  await button.focus();
  await p.keyboard.press("Enter");
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  // L'action qui a le focus se voit, comme dans les autres menus : un contour.
  await expect(item("Déplacer vers le haut")).toHaveCSS("outline-style", "solid");
  await expect(item("Déplacer vers le haut")).toHaveCSS("outline-width", "2px");
  await expect(item("Déplacer vers le bas")).toHaveCSS("outline-style", "none");
  await p.keyboard.press("ArrowDown");
  await expect(item("Déplacer vers le bas")).toBeFocused();
  await expect(item("Déplacer vers le bas")).toHaveCSS("outline-style", "solid");
  await p.keyboard.press("ArrowDown");
  await expect(item("Imbriquer dans la page précédente")).toBeFocused();
  // « Sortir de la page parente » n'a pas de sens à la racine : grisée, les flèches la passent.
  await expect(item("Sortir de la page parente")).toHaveAttribute("aria-disabled", "true");
  await p.keyboard.press("ArrowDown");
  await expect(item("Mettre à la corbeille")).toBeFocused();
  await p.keyboard.press("ArrowDown");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  await p.keyboard.press("End");
  await expect(item("Mettre à la corbeille")).toBeFocused();
  await p.keyboard.press("ArrowUp");
  await expect(item("Imbriquer dans la page précédente")).toBeFocused();
  await p.keyboard.press("Home");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  // Échap le ferme et rend le focus à son bouton.
  await p.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "false");

  // « Déplacer vers le haut » : la page monte d'un cran, sur le serveur aussi.
  await p.keyboard.press("Enter");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  await p.keyboard.press("Enter");
  await expect(menu).toHaveCount(0);
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Alpha", "1 Charlie"]);
  await expect(button).toBeFocused();
  const titles = async () =>
    byPosition((await getTree(api, space.id)).nodes).map((n) => n.title);
  expect(await titles()).toEqual(["Bravo", "Alpha", "Charlie"]);

  // En tête, elle ne monte pas plus haut : l'action est grisée, et le focus va à la suivante.
  await p.keyboard.press("Enter");
  await expect(item("Déplacer vers le haut")).toHaveAttribute("aria-disabled", "true");
  await expect(item("Déplacer vers le bas")).toBeFocused();
  // « Déplacer vers le bas », deux fois : elle finit dernière.
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  await p.keyboard.press("Enter");
  await p.keyboard.press("ArrowDown");
  await expect(item("Déplacer vers le bas")).toBeFocused();
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Charlie", "1 Bravo"]);
  expect(await titles()).toEqual(["Alpha", "Charlie", "Bravo"]);
  // Tab ferme le menu.
  await p.keyboard.press("Enter");
  await expect(menu).toBeVisible();
  await p.keyboard.press("Tab");
  await expect(menu).toHaveCount(0);
});

test("row_menu_nests_and_unnests_by_keyboard", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const c = await createPage(api, { title: "Charlie" });
  const d = await createPage(api, { parentId: c.id, title: "Delta" });
  const a = await createPage(api, { section: "private", title: "Alpha" });
  const b = await createPage(api, { section: "private", title: "Bravo" });
  const parentOf = async (id: string) =>
    (await getTree(api, space.id)).nodes.find((n) => n.id === id)?.parentId;

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  const start = ["1 Charlie", "2 Delta", "1 Alpha", "1 Bravo"];
  await expect.poll(() => shownRows(p)).toEqual(start);
  const button = treeRow(p, b.id).getByRole("button", { name: "Actions de « Bravo »" });
  const item = (title: string, name: string) =>
    p.getByRole("menu", { name: `Actions de « ${title} »` }).getByRole("menuitem", { name });
  const nest = "Imbriquer dans la page précédente";
  const unnest = "Sortir de la page parente";

  // Dans « Privé ». « Bravo », dernière : ni plus bas, ni hors d'une page parente.
  await button.focus();
  await p.keyboard.press("Enter");
  await expect(item("Bravo", "Déplacer vers le haut")).toBeFocused();
  await expect(item("Bravo", "Déplacer vers le bas")).toHaveAttribute("aria-disabled", "true");
  await expect(item("Bravo", unnest)).toHaveAttribute("aria-disabled", "true");
  // « Imbriquer dans la page précédente » : elle devient la sous-page de « Alpha », qui se
  // déplie ; le focus reste sur son bouton.
  await p.keyboard.press("ArrowDown");
  await expect(item("Bravo", nest)).toBeFocused();
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(["1 Charlie", "2 Delta", "1 Alpha", "2 Bravo"]);
  await expect(treeRow(p, a.id)).toHaveAttribute("aria-expanded", "true");
  expect(await parentOf(b.id)).toBe(a.id);
  await expect(button).toBeFocused();

  // Seule sous sa page parente : il ne lui reste qu'à en sortir, juste après elle.
  await p.keyboard.press("Enter");
  await expect(item("Bravo", unnest)).toBeFocused();
  for (const name of ["Déplacer vers le haut", "Déplacer vers le bas", nest]) {
    await expect(item("Bravo", name)).toHaveAttribute("aria-disabled", "true");
  }
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(start);
  expect(await parentOf(b.id)).toBeNull();
  expect(
    byPosition((await getTree(api, space.id)).nodes)
      .filter((n) => n.section === "private")
      .map((n) => n.title),
  ).toEqual(["Alpha", "Bravo"]);

  // Dans « Espace » aussi, une sous-page peut sortir à la racine : elle y garde son accès
  // général, et reste une page de l'espace (R94). Sans sœur avant elle, elle ne s'imbrique
  // nulle part.
  await openRowMenu(p, d.id, "Delta");
  await expect(item("Delta", unnest)).not.toHaveAttribute("aria-disabled", "true");
  await expect(item("Delta", nest)).toHaveAttribute("aria-disabled", "true");
  await p.keyboard.press("Escape");
  expect(await parentOf(d.id)).toBe(c.id);
});

test("subpage_dragged_to_the_space_root_stays_a_team_page", async ({ browser }) => {
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const parent = await createPage(aliceApi, { title: "Projet" });
  const child = await createPage(aliceApi, { parentId: parent.id, title: "Chapitre" });
  expect(await placesSeenBy(brunoApi, space.id)).toEqual({
    Projet: "racine, space",
    Chapitre: "sous-page, space",
  });

  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(a)).toEqual(["1 Projet", "2 Chapitre"]);
  // « Chapitre », lâchée sur le bord haut de « Projet », sort à la racine, avant elle.
  await treeRow(a, child.id).dragTo(treeRow(a, parent.id), { targetPosition: { x: 80, y: 2 } });
  await expect.poll(() => shownRows(a)).toEqual(["1 Chapitre", "1 Projet"]);
  await expect(sectionOf(a, "Espace").locator(`[data-page-id="${child.id}"]`)).toBeVisible();
  await expect(sidebarOf(a).getByRole("alert")).toHaveCount(0);

  // Elle reste une page d'équipe (R94) : l'autre membre la voit toujours, dans « Espace ».
  expect(await placesSeenBy(brunoApi, space.id)).toEqual({
    Projet: "racine, space",
    Chapitre: "racine, space",
  });
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await expect(sectionOf(b, "Espace").locator(`[data-page-id="${child.id}"]`)).toHaveAttribute(
    "aria-level",
    "1",
  );
  await expect.poll(() => shownRows(b)).toEqual(["1 Chapitre", "1 Projet"]);
});

test("subpage_unnested_by_keyboard_to_the_space_root_stays_a_team_page", async ({ browser }) => {
  const { alice, aliceApi, space, brunoApi } = await ownerAndMember();
  const parent = await createPage(aliceApi, { title: "Projet" });
  const child = await createPage(aliceApi, { parentId: parent.id, title: "Chapitre" });
  // Dans « Partagé », pour A : une page privée de B et sa sous-page, partagées avec elle en
  // accès complet.
  const { user } = await getMe(aliceApi);
  const theirs = await createPage(brunoApi, { spaceId: space.id, section: "private", title: "Dossier" });
  const piece = await createPage(brunoApi, { parentId: theirs.id, title: "Pièce" });
  await setPageLevel(brunoApi, theirs.id, user.id, "full");
  const unnest = "Sortir de la page parente";
  const item = (title: string, name: string) =>
    a.getByRole("menu", { name: `Actions de « ${title} »` }).getByRole("menuitem", { name });

  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}`);
  const start = ["1 Projet", "2 Chapitre", "1 Dossier", "2 Pièce"];
  await expect.poll(() => shownRows(a)).toEqual(start);

  // « Sortir de la page parente », au clavier : seule sous sa page parente, c'est la seule
  // action de déplacement qui lui reste, et elle a le focus à l'ouverture du menu.
  await treeRow(a, child.id).getByRole("button", { name: "Actions de « Chapitre »" }).focus();
  await a.keyboard.press("Enter");
  await expect(item("Chapitre", unnest)).toBeFocused();
  await a.keyboard.press("Enter");
  await expect.poll(() => shownRows(a)).toEqual(["1 Projet", "1 Chapitre", "1 Dossier", "2 Pièce"]);
  await expect(sidebarOf(a).getByRole("alert")).toHaveCount(0);
  // Elle reste une page d'équipe : l'autre membre la voit toujours, dans « Espace ».
  expect(await placesSeenBy(brunoApi, space.id)).toMatchObject({
    Projet: "racine, space",
    Chapitre: "racine, space",
  });

  // Dans « Partagé », rien ne change : sortie à la racine, la sous-page quitterait sa section.
  // L'action est grisée, et le dépôt à la souris refusé.
  await openRowMenu(a, piece.id, "Pièce");
  await expect(item("Pièce", unnest)).toHaveAttribute("aria-disabled", "true");
  await a.keyboard.press("Escape");
  await treeRow(a, piece.id).dragTo(treeRow(a, theirs.id), { targetPosition: { x: 80, y: 2 } });
  // Le déplacement suivant, lui, est fait : quand il se voit, le précédent aurait eu lieu.
  await treeRow(a, child.id).dragTo(treeRow(a, parent.id), { targetPosition: { x: 80, y: 2 } });
  await expect.poll(() => shownRows(a)).toEqual(["1 Chapitre", "1 Projet", "1 Dossier", "2 Pièce"]);
  expect((await getTree(aliceApi, space.id)).nodes.find((n) => n.id === piece.id)?.parentId).toBe(
    theirs.id,
  );
});

test("drop_target_is_shown_while_dragging", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });
  const before = (await getTree(api, space.id)).nodes;

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const box = async (id: string) => (await treeRow(p, id).boundingBox())!;
  const line = sidebarOf(p).locator(".tree-drop-line");

  // « Charlie » est saisie : sa ligne s'efface.
  const from = await box(c.id);
  await p.mouse.move(from.x + 80, from.y + from.height / 2);
  await p.mouse.down();
  await p.mouse.move(from.x + 86, from.y + from.height / 2 + 4, { steps: 3 });
  await expect(treeRow(p, c.id)).toHaveClass(/is-dragging/);

  // Dès que le pointeur arrive au milieu de « Bravo », elle s'éclaire : la page deviendrait
  // sa sous-page.
  const onto = await box(b.id);
  await p.mouse.move(onto.x + 80, onto.y + onto.height / 2);
  await expect(treeRow(p, b.id)).toHaveClass(/is-drop-inside/);
  await expect(line).toHaveCount(0);

  // Sur le bord haut de « Alpha » : un trait, au-dessus d'elle ; plus aucune ligne éclairée.
  const first = await box(a.id);
  await p.mouse.move(first.x + 80, first.y + 2);
  await expect(line).toHaveCount(1);
  expect(Math.abs((await line.boundingBox())!.y - first.y)).toBeLessThanOrEqual(2);
  await expect(sidebarOf(p).locator(".is-drop-inside")).toHaveCount(0);

  // Sur le bord bas de « Bravo », juste au-dessus d'elle-même : rien ne changerait, et le
  // milieu de « Bravo » reste permis ; sur elle-même : rien.
  await p.mouse.move(from.x + 80, from.y + from.height / 2);
  await expect(line).toHaveCount(0);
  await expect(sidebarOf(p).locator(".is-drop-inside")).toHaveCount(0);

  // Lâchée hors de la barre latérale : rien ne bouge, et plus rien n'est marqué.
  await p.mouse.move(700, 400, { steps: 3 });
  await p.mouse.up();
  await expect(sidebarOf(p).locator(".is-dragging")).toHaveCount(0);
  await expect(line).toHaveCount(0);
  expect(await shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  expect((await getTree(api, space.id)).nodes).toEqual(before);
});

test("drop_line_is_drawn_where_the_page_will_go", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  // Dans « Privé », où une sous-page peut passer à la racine.
  const parent = await createPage(api, { section: "private", title: "Parente" });
  await createPage(api, { parentId: parent.id, title: "Un" });
  const last = await createPage(api, { parentId: parent.id, title: "Deux" });
  await createPage(api, { section: "private", title: "Suivante" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect
    .poll(() => shownRows(p))
    .toEqual(["1 Parente", "2 Un", "2 Deux", "1 Suivante"]);
  const box = async (id: string) => (await treeRow(p, id).boundingBox())!;
  const line = sidebarOf(p).locator(".tree-drop-line");

  // « Deux », dernière sous-page de « Parente », est saisie et portée sur la moitié basse de
  // sa page parente, dépliée. Dedans, rien ne changerait : elle en sortirait, pour se ranger
  // après elle, donc après toutes ses sous-pages.
  const from = await box(last.id);
  await p.mouse.move(from.x + 80, from.y + from.height / 2);
  await p.mouse.down();
  await p.mouse.move(from.x + 86, from.y + from.height / 2 + 4, { steps: 3 });
  await expect(treeRow(p, last.id)).toHaveClass(/is-dragging/);
  const onto = await box(parent.id);
  await p.mouse.move(onto.x + 80, onto.y + onto.height * 0.6, { steps: 3 });
  await expect(line).toHaveCount(1);
  await expect(sidebarOf(p).locator(".is-drop-inside")).toHaveCount(0);
  // Le trait est là où la page ira : sous la dernière ligne du sous-arbre, et pas juste sous
  // « Parente » ; au retrait de « Parente » (son titre commence 22 px après le bord de sa
  // ligne), pas à celui de ses sous-pages, 14 px plus loin.
  const drawn = (await line.boundingBox())!;
  expect(Math.abs(drawn.y - (from.y + from.height))).toBeLessThanOrEqual(2);
  expect(Math.abs(drawn.x - (onto.x + 22))).toBeLessThanOrEqual(1);

  // Le pointeur quitte la barre latérale, la page toujours tenue : plus rien n'est marqué.
  await p.mouse.move(700, 400, { steps: 3 });
  await expect(line).toHaveCount(0);
  await p.mouse.up();
  await expect(sidebarOf(p).locator(".is-dragging")).toHaveCount(0);
  expect(await shownRows(p)).toEqual(["1 Parente", "2 Un", "2 Deux", "1 Suivante"]);
});

test("drop_target_does_not_need_related_target", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  // Les événements d'un glisser, tels qu'un navigateur qui ne dit pas d'où vient ni où va le
  // pointeur les envoie : sans `relatedTarget`. Le pointeur est au milieu de la ligne visée.
  const dataTransfer = await p.evaluateHandle(() => new DataTransfer());
  const fire = async (type: string, id: string) => {
    const box = (await treeRow(p, id).boundingBox())!;
    await treeRow(p, id).dispatchEvent(type, {
      dataTransfer,
      clientX: box.x + 80,
      clientY: box.y + box.height / 2,
    });
  };
  const inside = sidebarOf(p).locator(".is-drop-inside");

  await fire("dragstart", c.id);
  await expect(treeRow(p, c.id)).toHaveClass(/is-dragging/);
  await fire("dragenter", b.id);
  await expect(treeRow(p, b.id)).toHaveClass(/is-drop-inside/);
  // De « Bravo » à « Alpha » : le navigateur annonce l'entrée sur la nouvelle ligne, puis la
  // sortie de l'ancienne. Cette sortie n'est pas celle de l'arbre : « Alpha » reste éclairée.
  await fire("dragenter", a.id);
  await expect(treeRow(p, a.id)).toHaveClass(/is-drop-inside/);
  await fire("dragleave", b.id);
  await p.waitForTimeout(100);
  await expect(treeRow(p, a.id)).toHaveClass(/is-drop-inside/);
  // La sortie de la dernière ligne où le pointeur est entré : il a quitté l'arbre.
  await fire("dragleave", a.id);
  await expect(inside).toHaveCount(0);
  // Revenu, puis le glisser abandonné : plus rien n'est marqué.
  await fire("dragenter", b.id);
  await expect(treeRow(p, b.id)).toHaveClass(/is-drop-inside/);
  await fire("dragend", c.id);
  await expect(inside).toHaveCount(0);
  await expect(sidebarOf(p).locator(".is-dragging")).toHaveCount(0);
});

test("drop_line_goes_when_its_row_leaves", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  // L'horloge de la page est la nôtre : l'arbre ne se relit qu'après cinq secondes (R97).
  await p.clock.install();
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const line = sidebarOf(p).locator(".tree-drop-line");
  const dataTransfer = await p.evaluateHandle(() => new DataTransfer());
  /** Un événement du glisser sur la ligne `id`, le pointeur à `at` de sa hauteur. */
  const fire = async (type: string, id: string, at = 0.5) => {
    const box = (await treeRow(p, id).boundingBox())!;
    await treeRow(p, id).dispatchEvent(type, {
      dataTransfer,
      clientX: box.x + 80,
      clientY: box.y + box.height * at,
    });
  };

  // « Charlie » est tenue sur le bord bas d'« Alpha » : un trait, sous « Alpha ».
  await fire("dragstart", c.id);
  await fire("dragenter", a.id, 0.95);
  await expect(line).toHaveCount(1);

  // Ailleurs, « Alpha » part à la corbeille ; l'onglet reprend le focus, l'arbre est relu : la
  // ligne visée n'est plus là.
  expect((await api.post(`/api/v1/pages/${a.id}/trash`)).status()).toBe(200);
  await p.clock.fastForward(5_500);
  await p.evaluate(() => window.dispatchEvent(new Event("focus")));
  // Le trait part avec elle, et rien d'autre : la barre latérale est toujours là.
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Charlie"]);
  await expect(line).toHaveCount(0);
  await expect(treeRow(p, c.id)).toHaveClass(/is-dragging/);
  await fire("dragend", c.id);
  await expect(sidebarOf(p).locator(".is-dragging")).toHaveCount(0);
});

test("drag_state_does_not_linger_when_the_tree_changes_under_it", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });
  const d = await createPage(api, { title: "Delta" });

  const p = await newUser(browser, alice);
  // L'horloge de la page est la nôtre : l'arbre ne se relit qu'après cinq secondes (R97).
  await p.clock.install();
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie", "1 Delta"]);
  const dataTransfer = await p.evaluateHandle(() => new DataTransfer());
  const fire = async (type: string, id: string) => {
    const box = (await treeRow(p, id).boundingBox())!;
    await treeRow(p, id).dispatchEvent(type, {
      dataTransfer,
      clientX: box.x + 80,
      clientY: box.y + box.height / 2,
    });
  };
  const comeBack = async () => {
    await p.clock.fastForward(5_500);
    await p.evaluate(() => window.dispatchEvent(new Event("focus")));
  };
  const inside = sidebarOf(p).locator(".is-drop-inside");
  const trashElsewhere = async (id: string) =>
    expect((await api.post(`/api/v1/pages/${id}/trash`)).status()).toBe(200);

  // « Delta » est tenue sur « Alpha », la dernière ligne où le pointeur est entré. Ailleurs,
  // « Alpha » part à la corbeille : sa ligne quitte le document, sans que le navigateur ait
  // annoncé de sortie.
  await fire("dragstart", d.id);
  await fire("dragenter", a.id);
  await expect(treeRow(p, a.id)).toHaveClass(/is-drop-inside/);
  await trashElsewhere(a.id);
  await comeBack();
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Charlie", "1 Delta"]);
  await expect(inside).toHaveCount(0);
  // Le pointeur survole maintenant « Bravo », remontée sous lui, puis quitte l'arbre : plus
  // rien n'est éclairé, bien que la dernière entrée annoncée ait été sur une ligne disparue.
  await fire("dragover", b.id);
  await expect(treeRow(p, b.id)).toHaveClass(/is-drop-inside/);
  await fire("dragleave", b.id);
  await expect(inside).toHaveCount(0);
  await expect(treeRow(p, d.id)).toHaveClass(/is-dragging/);

  // La page tenue part à son tour à la corbeille, ailleurs : sa ligne quitte le document, et
  // le navigateur ne dira jamais la fin du glisser. Restaurée, elle n'est plus « tenue ».
  await trashElsewhere(d.id);
  await comeBack();
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Charlie"]);
  expect((await api.post(`/api/v1/pages/${d.id}/restore`)).status()).toBe(200);
  await comeBack();
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Charlie", "1 Delta"]);
  await expect(sidebarOf(p).locator(".is-dragging")).toHaveCount(0);
  // Et plus rien ne s'éclaire sans un nouveau glisser.
  await fire("dragenter", c.id);
  await expect(inside).toHaveCount(0);
});
