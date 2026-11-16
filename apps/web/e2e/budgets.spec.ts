// Budgets du §5.10 dans le navigateur. « Ouvrir une page jamais vue (réseau local)
// < 300 ms » vaut pour les pages jusqu'à 1 000 blocs ; une page de 10 000 blocs doit
// seulement s'ouvrir sans geler l'onglet, en moins de 3 s (décision validée par Alex, note
// du §5.10 : le coût d'une grande page est la liaison y-prosemirror, pas Yjs).
//
// Ce qui est mesuré : l'application est déjà chargée, sur une autre page, dans un onglet
// neuf ; le chronomètre part au clic sur la page dans l'arbre (navigation interne) et
// s'arrête quand la première image où le premier bloc de la page est dans l'éditeur,
// synchronisé avec le serveur (`data-synced`) et modifiable (`data-editable`), a fini son
// rendu (styles, mise en page et peinture compris). Chaque ouverture vise une page que
// l'onglet n'a jamais ouverte, et que le serveur n'a plus en mémoire : il la relit dans
// SQLite, comme une page que personne n'a ouverte depuis un moment.
//
// Où : sur ce que les gens utiliseront, un build de production (`vite build`, servi par
// `vite preview`) et un serveur compilé en release (`playwright.budgets.config.ts`,
// `corepack pnpm -C apps/web e2e:budgets`, Ruling R38). `window.__mementum` n'existe pas
// en production : la mesure lit les attributs du conteneur de l'éditeur.
//
// Seule la durée totale est affirmée. L'image la plus longue (`longestFrame`) est mesurée
// et affichée, jamais affirmée : la conception définit « sans geler l'onglet » par « moins
// de 3 s » (§5.10), pas par une durée d'image. Ces tests ne garantissent donc aucune
// fluidité pendant l'ouverture.
//
// Les premières mesures se font avec un petit arbre (les pages de mesure). Les dernières,
// avec 1 000 pages de plus dans l'arbre (Ruling R43) : l'ouverture ne doit pas suivre le
// nombre de pages de la barre latérale.
import { expect, test, type APIRequestContext, type Browser } from "@playwright/test";
import {
  closeContexts,
  createPage,
  getTree,
  newUser,
  percentile,
  putMmd,
  unique,
  waitSynced,
} from "./helpers";

/**
 * L'acteur d'une page s'arrête après 30 s sans connexion ni demande (`idle_close` de
 * `SyncConfig`, crate `mementum-sync`, que `mementum serve` prend par défaut) : après ce
 * délai, le serveur relit la page dans SQLite à la prochaine ouverture. Le test Rust
 * `e2e_wait_follows_idle_close` (`crates/mementum-sync/tests/budgets.rs`) lit cette
 * constante et échoue si elle ne vaut plus `idle_close`.
 */
const SERVER_IDLE_CLOSE_MS = 30_000;

/** Ouvertures mesurées pour le p95 des pages de 1 000 blocs. */
const OPENINGS = 5;

/** Pages vides ajoutées à la racine de l'arbre pour les dernières mesures (R43). */
const TREE_PAGES = 1_000;

/** Créations de ces pages envoyées en même temps. */
const CREATE_CONCURRENCY = 8;

/** Les lignes de l'arbre (une par page affichée) : il n'en faut que ce qui se voit. */
const MAX_TREE_ROWS = 100;

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
  /**
   * Du clic dans l'arbre à la fin du rendu de la première image où la page est ouverte
   * (ms).
   */
  total: number;
  /**
   * Plus long intervalle pendant l'ouverture entre deux images, ou entre la dernière et la
   * fin de son rendu (ms) : combien de temps l'onglet reste figé.
   */
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
        // L'éditeur montre le premier bloc de la page (pas celui de la page quittée), sa
        // première synchro est faite, et il accepte la frappe. Le texte du premier bloc
        // est propre à chaque page, et le conteneur repasse à `data-synced="false"` dès que
        // l'éditeur prend la session d'une autre page, avant d'en rien montrer : les deux
        // attributs sont donc ceux de cette page.
        const opened = () => {
          const container = document.querySelector(".mm-editor");
          if (
            container?.getAttribute("data-synced") !== "true" ||
            container.getAttribute("data-editable") !== "true"
          ) {
            return false;
          }
          const editor = container.querySelector(".bn-editor");
          return (
            editor?.getAttribute("contenteditable") === "true" &&
            editor.querySelector(".bn-block-content")?.textContent === first
          );
        };
        let last = 0;
        let longestFrame = 0;
        const gap = (now: number) => {
          longestFrame = Math.max(longestFrame, now - last);
          last = now;
        };
        // Vérifié au début de chaque image. Les rappels `requestAnimationFrame` passent
        // avant le calcul des styles, la mise en page et la peinture de l'image : le
        // chronomètre s'arrête donc dans une tâche postée depuis ce rappel, qui ne s'exécute
        // qu'une fois l'image rendue (mise en page comprise).
        const rendered = new MessageChannel();
        rendered.port1.onmessage = () => {
          const end = performance.now();
          gap(end);
          rendered.port1.close();
          resolve({ total: end - t0, longestFrame });
        };
        const frame = () => {
          const now = performance.now();
          gap(now);
          if (opened()) rendered.port2.postMessage(null);
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

/**
 * Ouvre, chacune dans un onglet neuf, les pages de 1 000 blocs, et consigne les durées
 * (`what` : ce qui distingue la mesure dans la ligne `MESURE`). Renvoie le p95 (ms).
 */
async function openSmallPages(browser: Browser, what: string, targets: Target[] = small): Promise<number> {
  const totals: number[] = [];
  for (const target of targets) {
    const { total, longestFrame } = await openFromTree(browser, target);
    console.log(
      `  ouverture 1 000 blocs : ${Math.round(total)} ms (image la plus longue ${Math.round(longestFrame)} ms)`,
    );
    totals.push(total);
    await closeContexts();
  }
  const p95 = percentile(totals, 0.95);
  console.log(
    `MESURE ouverture d'une page jamais vue de 1 000 blocs${what} : p50 ${Math.round(percentile(totals, 0.5))} ms, ` +
      `p95 ${Math.round(p95)} ms (${totals.length} ouvertures : ${totals.map(Math.round).join(", ")})`,
  );
  expect(totals).toHaveLength(OPENINGS);
  return p95;
}

test("open_1000_block_page_under_300ms", async ({ browser }) => {
  expect(await openSmallPages(browser, "")).toBeLessThan(300);
});

/**
 * Pages de blocs riches (Task C1b, R346) : une page « mêlée » de 1 000 blocs, dont 100 blocs riches
 * (un bloc sur dix : un callout, puis un signet), tient le budget des 1 000 blocs ; une page de
 * 1 000 callouts est mesurée et consignée, sans budget à elle (la relecture finale de la partie B
 * en décide).
 */
test.describe("blocs riches", () => {
  let mixed: Target[] = [];
  let callouts: Target | null = null;

  /** Une page de 1 000 blocs, dont le bloc `n` est `rich(n)` s'il en rend un, sinon un paragraphe. */
  async function richPage(request: APIRequestContext, rich: (n: number) => string | null): Promise<Target> {
    const tag = unique();
    const { id } = await createPage(request, { title: `Mesure blocs riches ${tag}` });
    const blocks = Array.from({ length: 1_000 }, (_, i) => rich(i + 1) ?? blockText(tag, i + 1));
    await putMmd(request, id, `${blocks.join("\n\n")}\n`);
    return { id, first: blockText(tag, 1) };
  }

  test.beforeAll(async ({ playwright }) => {
    test.setTimeout(180_000);
    const request = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL });
    try {
      mixed = [];
      for (let i = 0; i < OPENINGS; i++) {
        mixed.push(
          await richPage(request, (n) =>
            n % 10 !== 0
              ? null
              : n % 20 === 0
                ? `<bookmark url="https://exemple.fr/page/${n}"/>`
                : `<callout icon="⚠️" color="blue">\nCallout ${n} : un texte à faire ressortir.\n</callout>`,
          ),
        );
      }
      // Le premier bloc est un callout aussi : son texte est celui que l'ouverture attend.
      const page = await richPage(request, (n) => `<callout icon="💡" color="default">\nCallout ${n}\n</callout>`);
      callouts = { id: page.id, first: "Callout 1" };
    } finally {
      await request.dispose();
    }
    await new Promise((done) => setTimeout(done, SERVER_IDLE_CLOSE_MS + 2_000));
  });

  test("open_1000_block_page_with_100_rich_blocks_under_300ms", async ({ browser }) => {
    expect(await openSmallPages(browser, " dont 100 blocs riches", mixed)).toBeLessThan(300);
  });

  test("measure_open_1000_callout_page", async ({ browser }) => {
    const { total, longestFrame } = await openFromTree(browser, callouts!);
    console.log(
      `MESURE ouverture d'une page jamais vue de 1 000 callouts : ${Math.round(total)} ms ` +
        `(image la plus longue ${Math.round(longestFrame)} ms)`,
    );
  });
});

/**
 * Une page de formules (Task C2, R423) : cinquante équations de 10 000 caractères, sans macro,
 * après un paragraphe. Leur mise en forme figeait l'ouverture 22 s ; une formule n'est plus mise en
 * forme qu'à l'écran, l'une après l'autre, quelques millisecondes par image : l'ouverture tient le
 * budget des 1 000 blocs, et l'image la plus longue est consignée.
 */
test.describe("formules", () => {
  let heavy: Target[] = [];

  test.beforeAll(async ({ playwright }) => {
    test.setTimeout(180_000);
    const request = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL });
    try {
      heavy = [];
      for (let i = 0; i < OPENINGS; i++) {
        const tag = unique();
        const { id } = await createPage(request, { title: `Mesure formules ${tag}` });
        const formula = Array.from({ length: 400 }, (_, k) => `\\frac{a_{${k}}}{b^{2}}`).join(" + ");
        const blocks = [blockText(tag, 1), ...Array.from({ length: 50 }, () => `$$\n${formula}\n$$`)];
        await putMmd(request, id, `${blocks.join("\n\n")}\n`);
        heavy.push({ id, first: blockText(tag, 1) });
      }
    } finally {
      await request.dispose();
    }
    await new Promise((done) => setTimeout(done, SERVER_IDLE_CLOSE_MS + 2_000));
  });

  test("open_page_of_50_long_formulas_under_300ms", async ({ browser }) => {
    expect(await openSmallPages(browser, " : cinquante formules de 10 000 caractères", heavy)).toBeLessThan(300);
  });
});

test("open_10000_block_page_under_3s", async ({ browser }) => {
  const { total, longestFrame } = await openFromTree(browser, large!);
  console.log(
    `MESURE ouverture d'une page jamais vue de 10 000 blocs : ${Math.round(total)} ms ` +
      `(image la plus longue ${Math.round(longestFrame)} ms)`,
  );
  expect(total).toBeLessThan(3_000);
});

test.describe("arbre de 1 000 pages", () => {
  /** La dernière page de l'arbre, tout en bas de la barre latérale. */
  let last: { id: string; title: string } = { id: "", title: "" };

  test.beforeAll(async ({ playwright }) => {
    test.setTimeout(180_000);
    const request = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL });
    try {
      const tag = unique();
      for (let done = 0; done < TREE_PAGES; done += CREATE_CONCURRENCY) {
        const batch = Math.min(CREATE_CONCURRENCY, TREE_PAGES - done);
        await Promise.all(
          Array.from({ length: batch }, (_, i) =>
            createPage(request, { title: `Arbre ${tag} ${done + i + 1}` }),
          ),
        );
      }
      const { nodes } = await getTree(request);
      expect(nodes.length).toBeGreaterThanOrEqual(TREE_PAGES);
      // Ordre des frères : `position`, comparée unité par unité (ASCII).
      const roots = nodes
        .filter((n) => n.parentId === null)
        .sort((a, b) => (a.position < b.position ? -1 : a.position > b.position ? 1 : 0));
      const bottom = roots[roots.length - 1];
      expect(bottom.title).toContain(`Arbre ${tag} `);
      last = { id: bottom.id, title: bottom.title };
    } finally {
      await request.dispose();
    }
    // Les pages de 1 000 blocs ont été ouvertes par la première mesure : on attend que le
    // serveur les ait quittées, comme celles que l'on vient de créer.
    await new Promise((done) => setTimeout(done, SERVER_IDLE_CLOSE_MS + 2_000));
  });

  test("open_page_with_1000_pages_in_tree_under_300ms", async ({ browser }) => {
    // Les mêmes pages que `open_1000_block_page_under_300ms` : seul l'arbre a changé.
    expect(await openSmallPages(browser, ", arbre de 1 000 pages de plus")).toBeLessThan(300);
  });

  test("tree_renders_only_visible_rows", async ({ browser }) => {
    const p = await newUser(browser);
    await p.goto(`/p/${landing}`);
    await waitSynced(p);
    const sidebar = p.getByRole("navigation", { name: "Barre latérale" });
    await expect(sidebar.locator(`[data-page-id="${landing}"]`)).toBeInViewport();
    // Une ligne d'arbre par élément `data-page-id`, quelle que soit la forme de l'arbre.
    const rows = p.locator("[data-page-id]");
    const atTop = await rows.count();
    console.log(`MESURE lignes de l'arbre dans le document, en haut : ${atTop}`);
    expect(atTop).toBeLessThan(MAX_TREE_ROWS);

    await sidebar.evaluate((el) => el.scrollTo(0, el.scrollHeight));
    const bottom = sidebar.locator(`[data-page-id="${last.id}"]`);
    await expect(bottom).toBeInViewport();
    const atBottom = await rows.count();
    console.log(`MESURE lignes de l'arbre dans le document, en bas : ${atBottom}`);
    expect(atBottom).toBeLessThan(MAX_TREE_ROWS);

    await bottom.getByRole("link").click();
    await expect(p).toHaveURL(new RegExp(`/p/${last.id}$`));
    await expect(p.getByRole("textbox", { name: "Titre de la page" })).toHaveValue(last.title);
  });

  test("active_page_far_down_the_tree_is_shown", async ({ browser }) => {
    // Lien direct vers une page hors de la partie visible au chargement : l'arbre défile
    // jusqu'à elle.
    const p = await newUser(browser);
    await p.goto(`/p/${last.id}`);
    await waitSynced(p);
    const sidebar = p.getByRole("navigation", { name: "Barre latérale" });
    const row = sidebar.locator(`[data-page-id="${last.id}"]`);
    await expect(row).toBeInViewport();
    await expect(row.getByRole("link")).toHaveAttribute("aria-current", "page");
  });
});
