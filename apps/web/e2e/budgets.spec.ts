// Budgets du §5.10 dans le navigateur. « Ouvrir une page jamais vue (réseau local)
// < 300 ms » vaut pour les pages jusqu'à 1 000 blocs ; une page de 10 000 blocs doit
// seulement s'ouvrir sans geler l'onglet, en moins de 3 s (décision validée par Alex, note
// du §5.10 : le coût d'une grande page est la liaison y-prosemirror, pas Yjs).
//
// Ce qui est mesuré : l'application est déjà chargée, sur une autre page, dans un onglet
// neuf ; le chronomètre part au clic sur la page dans l'arbre (navigation interne) et
// s'arrête à la première image où le premier bloc de la page est affiché dans l'éditeur,
// synchronisé avec le serveur (`provider.synced`) et modifiable. Chaque ouverture vise une
// page que l'onglet n'a jamais ouverte, et que le serveur n'a plus en mémoire : il la relit
// dans SQLite, comme une page que personne n'a ouverte depuis un moment.
import { expect, test, type APIRequestContext, type Browser } from "@playwright/test";
import { closeContexts, createPage, newUser, percentile, putMmd, unique, waitSynced } from "./helpers";

/**
 * L'acteur d'une page s'arrête après 30 s sans connexion ni demande (`idle_close` de
 * `SyncConfig`, crate `mementum-sync`) : après ce délai, le serveur relit la page dans
 * SQLite à la prochaine ouverture.
 */
const SERVER_IDLE_CLOSE_MS = 30_000;

/** Ouvertures mesurées pour le p95 des pages de 1 000 blocs. */
const OPENINGS = 5;

type Target = { id: string; first: string };

let landing = "";
let small: Target[] = [];
let large: Target | null = null;

test.beforeAll(async ({ playwright }) => {
  test.setTimeout(180_000);
  const request = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL });
  try {
    landing = (await createPage(request, { title: `Accueil ${unique()}` })).id;
    small = [];
    for (let i = 0; i < OPENINGS; i++) small.push(await bigPage(request, 1_000));
    large = await bigPage(request, 10_000);
  } finally {
    await request.dispose();
  }
  // Les pages viennent d'être écrites : on attend que le serveur les ait quittées.
  await new Promise((done) => setTimeout(done, SERVER_IDLE_CLOSE_MS + 2_000));
});

test.afterEach(closeContexts);

/** Le texte du bloc `n` (à partir de 1) de la page `tag`. */
function blockText(tag: string, n: number): string {
  return `Page ${tag}, paragraphe ${n} : du texte ordinaire pour mesurer l'ouverture.`;
}

/** Une page de `blocks` paragraphes, écrite par l'API (`PUT /mmd`). */
async function bigPage(request: APIRequestContext, blocks: number): Promise<Target> {
  const tag = unique();
  const { id } = await createPage(request, { title: `Mesure ${blocks} blocs ${tag}` });
  // MMD canonique : une ligne vide entre deux blocs, un saut de ligne final.
  const mmd = `${Array.from({ length: blocks }, (_, i) => blockText(tag, i + 1)).join("\n\n")}\n`;
  await putMmd(request, id, mmd);
  return { id, first: blockText(tag, 1) };
}

type Opening = {
  /** Du clic dans l'arbre à la première image où la page est ouverte (ms). */
  total: number;
  /** Plus long intervalle entre deux images pendant l'ouverture (ms). */
  longestFrame: number;
};

/**
 * Charge l'application sur la page d'accueil dans un onglet neuf, puis ouvre `target` d'un
 * clic dans l'arbre et mesure l'ouverture dans la page, avec `performance.now()`.
 */
async function openFromTree(browser: Browser, target: Target): Promise<Opening> {
  const p = await newUser(browser);
  await p.goto(`/p/${landing}`);
  await waitSynced(p);
  // Application chargée et au repos : seule l'ouverture de la page est mesurée.
  await p.evaluate(() => new Promise((done) => requestIdleCallback(done, { timeout: 2_000 })));
  return p.evaluate(
    ({ id, first, timeout }) =>
      new Promise<Opening>((resolve, reject) => {
        const link = document.querySelector<HTMLAnchorElement>(`[data-page-id="${id}"] .tree-link`);
        if (!link) {
          reject(new Error(`page ${id} absente de l'arbre`));
          return;
        }
        // La session vise la page, sa première synchro est faite, et l'éditeur montre
        // le premier bloc de la page (pas celui de la page quittée) et accepte la frappe.
        const opened = () => {
          const m = window.__mementum;
          if (!m || m.provider.roomname !== id || !m.provider.synced) return false;
          const editor = document.querySelector(".bn-editor");
          return (
            editor?.getAttribute("contenteditable") === "true" &&
            editor.querySelector(".bn-block-content")?.textContent === first
          );
        };
        let last = 0;
        let longestFrame = 0;
        // Vérifié à chaque image : l'arrêt du chronomètre est la première image où la
        // page est affichée, après la mise à jour du DOM.
        const frame = () => {
          const now = performance.now();
          longestFrame = Math.max(longestFrame, now - last);
          last = now;
          if (opened()) resolve({ total: now - t0, longestFrame });
          else if (now - t0 > timeout) reject(new Error(`page ${id} pas ouverte après ${timeout} ms`));
          else requestAnimationFrame(frame);
        };
        const t0 = performance.now();
        last = t0;
        link.click();
        requestAnimationFrame(frame);
      }),
    { ...target, timeout: 30_000 },
  );
}

test("open_1000_block_page_under_300ms", async ({ browser }) => {
  const totals: number[] = [];
  for (const target of small) {
    const { total, longestFrame } = await openFromTree(browser, target);
    console.log(
      `  ouverture 1 000 blocs : ${Math.round(total)} ms (image la plus longue ${Math.round(longestFrame)} ms)`,
    );
    totals.push(total);
    await closeContexts();
  }
  const p95 = percentile(totals, 0.95);
  console.log(
    `MESURE ouverture d'une page jamais vue de 1 000 blocs : p50 ${Math.round(percentile(totals, 0.5))} ms, ` +
      `p95 ${Math.round(p95)} ms (${totals.length} ouvertures : ${totals.map(Math.round).join(", ")})`,
  );
  expect(totals).toHaveLength(OPENINGS);
  expect(p95).toBeLessThan(300);
});

test("open_10000_block_page_under_3s", async ({ browser }) => {
  const { total, longestFrame } = await openFromTree(browser, large!);
  console.log(
    `MESURE ouverture d'une page jamais vue de 10 000 blocs : ${Math.round(total)} ms ` +
      `(image la plus longue ${Math.round(longestFrame)} ms)`,
  );
  expect(total).toBeLessThan(3_000);
});
