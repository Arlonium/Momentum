// Un compte freiné sur une page l'apprend (R788 ; R781 (b)).
//
// Le serveur refuse après les avoir appliquées certaines mises à jour (R538), et chaque refus de
// ce genre recharge la page pour tous : après quelques-uns en une minute, le compte est freiné
// sur cette page (R725, R745), et ses connexions de synchro y sont fermées sans que rien soit
// appliqué. L'onglet revenait en boucle, sans rien dire, et ce qui s'y tapait n'était jamais
// enregistré. Le serveur envoie maintenant, avant de fermer, la trame de page `blocked` : l'onglet
// cesse de revenir, verrouille l'éditeur sans rien retirer de l'écran, et dit quoi faire.
import { expect, test } from "@playwright/test";
import { apiAs, closeContexts, createPage, getMmd, openPage, signUpAccount, unique } from "./helpers";
import { rawSync, varUint } from "./raw-sync";

test.afterEach(closeContexts);

/**
 * Une mise à jour que le serveur refuse après l'avoir appliquée : elle écrit dans les
 * métadonnées de la page (`writes_the_metadata`, `crates/mementum-sync/tests/sync.rs`).
 */
function writesTheMetadata(): Uint8Array<ArrayBuffer> {
  const update = [1, 1, 99, 0, 0x28, 1, 8, ...new TextEncoder().encode("mementum"), 1, 120, 1, 119, 1, 121, 0];
  return Uint8Array.from([0, 2, ...varUint(update.length), ...update]);
}

test("a_braked_account_is_told_that_nothing_it_types_is_saved", async ({ browser, baseURL }) => {
  // Un compte neuf : le frein est par compte et par page, et dure.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const page = await createPage(api, { title: `Frein ${unique()}` });
  const tab = await openPage(browser, page.id, alice);
  const editor = tab.locator(".bn-editor");
  await editor.click();
  await tab.keyboard.type("Avant");
  await expect.poll(() => getMmd(api, page.id)).toContain("Avant");

  const sockets: string[] = [];
  tab.on("websocket", (socket) => {
    if (socket.url().includes("/sync/")) sockets.push(socket.url());
  });
  const banner = tab.locator(".mm-banner-blocked");
  // Des connexions brutes du même compte se font refuser en boucle ; après chaque refus,
  // l'onglet tape. Le frein tombe au quatrième refus d'une minute, pour deux secondes, puis le
  // double à chaque récidive : l'onglet finit par taper pendant qu'il tient.
  let typed = "";
  await expect(async () => {
    const hostile = await rawSync(api, baseURL!, page.id);
    try {
      hostile.send(writesTheMetadata());
      expect(await hostile.settled()).toBe("fermée");
    } finally {
      hostile.close();
    }
    await tab.keyboard.type("x");
    typed += "x";
    await expect(banner).toBeVisible({ timeout: 700 });
  }).toPass({ timeout: 60_000, intervals: [0] });

  // En alerte, en clair, avec de quoi recharger.
  await expect(banner).toHaveAttribute("role", "alert");
  await expect(banner).toContainText("Tes dernières modifications sur cette page ne s'enregistrent plus.");
  await expect(banner).toContainText("Copie ce que tu viens d'écrire, puis recharge la page.");
  await expect(banner.getByRole("button", { name: "Recharger" })).toBeVisible();
  // L'éditeur est verrouillé, et ce qui a été tapé reste à l'écran, à copier.
  await expect(tab.locator(".mm-editor")).toHaveAttribute("data-editable", "false");
  await expect(editor).toContainText(`Avant${typed}`);
  await tab.keyboard.type("perdu");
  await expect(editor).not.toContainText("perdu");
  // La barre du haut ne promet pas que les modifications partiront au retour du réseau.
  await expect(tab.locator(".mm-topbar-status")).toHaveText("");
  // L'onglet ne revient plus : aucune connexion de synchro de plus (y-websocket reviendrait
  // après un dixième de seconde, puis le double).
  const opened = sockets.length;
  await tab.waitForTimeout(2_000);
  expect(sockets.length).toBe(opened);

  // Rechargée, la page est celle du serveur, sans ce qu'il n'a pas enregistré.
  await banner.getByRole("button", { name: "Recharger" }).click();
  await expect(tab.locator(".mm-editor")).toHaveAttribute("data-synced", "true", { timeout: 30_000 });
  await expect(tab.locator(".bn-editor")).toContainText("Avant");
  await expect(tab.locator(".mm-banner-blocked")).toHaveCount(0);
});
