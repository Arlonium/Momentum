// Un lien de plus de 16 Kio n'est pas posé (R495) : l'éditeur refuse la transaction et le dit, par
// la zone d'alerte commune aux gestes refusés (`editor/editorNotice.ts`, R641). Le serveur, lui,
// refuserait la mise à jour, et l'onglet la renverrait à chaque reconnexion.
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, getMmd, openPage, ownerAndMember, putMmd, unique } from "./helpers";

test.afterEach(closeContexts);

/**
 * Sélectionne « lien » dans « Un lien ici », au clavier (deux doubles clics de suite au même endroit
 * en font un triple, qui prend tout le paragraphe), puis lui donne le lien `href` par la barre de mise
 * en forme.
 */
async function linkWord(p: Page, href: string): Promise<void> {
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.press("End");
  for (let i = 0; i < " ici".length; i += 1) await p.keyboard.press("ArrowLeft");
  for (let i = 0; i < "lien".length; i += 1) await p.keyboard.press("Shift+ArrowLeft");
  await p.getByRole("button", { name: "Créer un lien" }).click();
  const field = p.getByPlaceholder("Modifier l'URL");
  await field.fill(href);
  await field.press("Enter");
}

test("a_link_over_16_kib_is_refused_and_said", async ({ browser }) => {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Lien ${unique()}` });
  await putMmd(team.aliceApi, page.id, "Un lien ici\n");
  const mona = await openPage(browser, page.id, team.alice);

  // Une adresse d'un octet de plus que la borne : rien n'est posé, et l'alerte le dit.
  await linkWord(mona, `https://exemple.fr/${"a".repeat(16 * 1024)}`);
  const alert = mona.locator(".mm-link-refused[role=alert]");
  await expect(alert).toHaveText("Lien trop long : une adresse s'écrit en 16 Kio au plus. Le lien n'a pas été posé.");
  await expect(mona.locator(".mm-editor a[href]")).toHaveCount(0);

  // Témoin : une adresse ordinaire est posée, et le serveur la relit.
  await linkWord(mona, "https://mementum.dev/");
  await expect(mona.locator(".mm-editor a[href='https://mementum.dev/']")).toHaveText("lien");
  await expect.poll(() => getMmd(team.aliceApi, page.id)).toBe("Un [lien](https://mementum.dev/) ici\n");
});
