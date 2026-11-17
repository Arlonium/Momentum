// Les fichiers d'une page (Task C8, R245 à R248) : déposer, coller ou choisir un fichier l'envoie
// dans la page ; une image, une vidéo ou un son s'y affichent, tout autre fichier se télécharge,
// aussi pour qui ne fait que lire ; un fichier dont le type ment ne s'exécute jamais (preuve 1 du
// plan) ; un fichier cesse d'être servi dès que sa page n'est plus lisible (preuve 2).
//
// Le serveur de cette suite n'admet qu'un Mio par fichier (`E2E_MAX_FILE_MB`).
import { expect, test, type APIRequestContext, type Browser, type Locator, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  apiAs,
  closeContexts,
  closeEventsChannel,
  createPage,
  forbidNativeDialogs,
  getMmd,
  hideSyncClosures,
  newUser,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  treeRow,
  unique,
  versionsOf,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

const fixture = (name: string) => readFileSync(fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url)));
const PNG = fixture("photo.png");
const HTML = fixture("html-named.png");
const VIDEO = fixture("video.webm");

const FILE_URL = /^\/api\/v1\/pages\/[0-9a-f-]{36}\/files\/[0-9a-f-]{36}$/;

type Dropped = { name: string; bytes: Buffer; type: string };

/** Un `DataTransfer` de la page qui porte `files`, comme un dépôt ou un collage venu du système. */
function transfer(p: Page, files: Dropped[]) {
  return p.evaluateHandle(
    (given) => {
      const data = new DataTransfer();
      for (const { name, base64, type } of given) {
        const raw = atob(base64);
        const bytes = Uint8Array.from(raw, (c) => c.charCodeAt(0));
        data.items.add(new File([bytes], name, { type }));
      }
      return data;
    },
    files.map(({ name, bytes, type }) => ({ name, base64: bytes.toString("base64"), type })),
  );
}

/** Dépose `files` au milieu de `on`. */
async function drop(p: Page, on: Locator, ...files: Dropped[]): Promise<void> {
  const box = (await on.boundingBox())!;
  const dataTransfer = await transfer(p, files);
  await on.dispatchEvent("drop", {
    dataTransfer,
    clientX: box.x + box.width / 2,
    clientY: box.y + box.height / 2,
    bubbles: true,
    cancelable: true,
  });
}

/** Le premier bloc de l'éditeur de la page. */
const firstBlock = (p: Page) => p.locator(".mm-editor .bn-block-content").first();
const images = (p: Page) => p.locator(".mm-editor img.bn-visual-media");
/** Les refus d'envoi que la page montre. */
const refusals = (p: Page) => p.locator('.mm-file-refusals[role="alert"] p');
const uploads = (p: Page) => p.locator(".mm-file-uploads li");

/** L'image `image` est chargée et montrée (pas l'icône d'une image cassée). */
async function expectShown(image: Locator): Promise<void> {
  await expect(image).toBeVisible();
  await expect
    .poll(() => image.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth), { timeout: 10_000 })
    .toBeGreaterThan(0);
}

/** Envoie un fichier dans la page par l'API, comme un agent ; rend son adresse. */
async function upload(api: APIRequestContext, pageId: string, name: string, bytes: Buffer): Promise<string> {
  const res = await api.post(`/api/v1/pages/${pageId}/files?name=${encodeURIComponent(name)}`, {
    data: bytes,
    headers: { "content-type": "application/octet-stream" },
  });
  expect(res.status(), await res.text()).toBe(201);
  return ((await res.json()) as { url: string }).url;
}

/** Les envois de fichiers partis de l'onglet. */
function watchUploads(p: Page): string[] {
  const sent: string[] = [];
  p.on("request", (request) => {
    if (request.method() === "POST" && /\/api\/v1\/pages\/[^/]+\/files\?/.test(request.url())) sent.push(request.url());
  });
  return sent;
}

/** Mona (propriétaire) et Tom (membre) sur une page de « Espace », vide. */
async function duo(browser: Browser, title: string) {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `${title} ${unique()}` });
  const mona = await openPage(browser, page.id, team.alice);
  const tom = await openPage(browser, page.id, team.bruno);
  return { ...team, page, mona, tom };
}

/** Une page privée d'Alice, où Bruno a le niveau `level`. */
async function sharedPrivate(level: "read" | "comment" | "edit", title: string) {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, {
    spaceId: team.space.id,
    section: "private",
    title: `${title} ${unique()}`,
  });
  await setPageLevel(team.aliceApi, page.id, team.brunoId, level);
  return { ...team, page };
}

test("image_dropped_in_the_page_is_seen_by_the_other_at_once", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Image");
  const consoles: string[] = [];
  for (const p of [mona, tom]) p.on("pageerror", (error) => consoles.push(error.message));
  await drop(mona, firstBlock(mona), { name: "photo.png", bytes: PNG, type: "image/png" });

  // Mona la voit, puis Tom, sans rien faire : l'image est chargée chez chacun, avec sa session.
  await expectShown(images(mona));
  await expectShown(images(tom));
  const src = await images(tom).getAttribute("src");
  expect(src).toMatch(FILE_URL);
  expect(src).toContain(`/pages/${page.id}/files/`);
  expect(await images(mona).getAttribute("src")).toBe(src);
  // Plus d'envoi en cours, et rien de refusé.
  await expect(uploads(mona)).toHaveCount(0);
  await expect(refusals(mona)).toHaveCount(0);

  // Le serveur la relit en MMD, tel que le §6.2 l'écrit ; ce MMD, récrit, laisse la page telle quelle.
  const mmd = await getMmd(aliceApi, page.id);
  expect(mmd).toBe(`![](${src}) {name="photo.png"}\n`);
  await putMmd(aliceApi, page.id, mmd);
  expect(await getMmd(aliceApi, page.id)).toBe(mmd);
  await expectShown(images(tom));
  expect(consoles).toEqual([]);
});

test("file_is_downloaded_by_a_reader_who_cannot_upload", async ({ browser }) => {
  const { alice, bruno, page } = await sharedPrivate("read", "Fichier");
  const mona = await openPage(browser, page.id, alice);
  const bytes = Buffer.from("Un rapport, en texte.\n".repeat(50), "utf8");
  // Mona colle un fichier qui n'est ni image, ni vidéo, ni son : un bloc fichier.
  await firstBlock(mona).click();
  const pasted = await transfer(mona, [{ name: "rapport final.txt", bytes, type: "text/plain" }]);
  await mona.locator(".mm-editor .bn-editor").evaluate((editor, clipboardData) => {
    editor.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
  }, pasted);
  const monaLink = mona.locator(".mm-editor a.mm-file-download");
  await expect(monaLink).toHaveText("Télécharger");

  // Tom ne fait que lire : il voit le fichier, et peut le télécharger.
  const tom = await newUser(browser, bruno);
  const tomUploads = watchUploads(tom);
  await tom.goto(`/p/${page.id}`);
  await expect(readerBanner(tom)).toBeVisible();
  const link = tom.locator(".mm-editor a.mm-file-download");
  await expect(link).toHaveAccessibleName(/^Télécharger .*rapport final\.txt/);
  await expect(tom.locator(".mm-editor .bn-file-name")).toHaveText("rapport final.txt");
  const here = tom.url();
  const [download] = await Promise.all([tom.waitForEvent("download"), link.click()]);
  expect(download.suggestedFilename()).toBe("rapport final.txt");
  expect(readFileSync(await download.path()).equals(bytes)).toBe(true);
  // Sans quitter la page.
  expect(tom.url()).toBe(here);

  // Il n'envoie rien : ni dépôt, ni collage, ni bouton pour ajouter un fichier.
  const before = await tom.locator(".mm-editor .bn-block-outer").count();
  await drop(tom, tom.locator(".mm-editor .bn-block-content").last(), { name: "photo.png", bytes: PNG, type: "image/png" });
  const again = await transfer(tom, [{ name: "photo.png", bytes: PNG, type: "image/png" }]);
  await tom.locator(".mm-editor .bn-editor").evaluate((editor, clipboardData) => {
    editor.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
  }, again);
  // Témoin et barrière : le même dépôt, fait par Mona, ajoute une image, que Tom reçoit.
  await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name: "photo.png", bytes: PNG, type: "image/png" });
  await expectShown(images(tom));
  expect(await tom.locator(".mm-editor .bn-block-outer").count()).toBe(before + 1);
  expect(tomUploads).toEqual([]);
  await expect(tom.locator(".mm-editor .bn-add-file-button")).toHaveCount(0);
  await expect(uploads(tom)).toHaveCount(0);
});

test("html_named_png_never_runs", async ({ browser }) => {
  const { mona, tom } = await duo(browser, "Piège");
  const monaDialogs = forbidNativeDialogs(mona);
  const tomDialogs = forbidNativeDialogs(tom);
  // Un HTML à script, nommé `photo.png` et déclaré `image/png` : l'éditeur le prend pour une image.
  await drop(mona, firstBlock(mona), { name: "photo.png", bytes: HTML, type: "image/png" });

  // Le serveur y lit autre chose qu'une image : le bloc devient un bloc fichier, à télécharger.
  for (const p of [mona, tom]) {
    await expect(p.locator(".mm-editor a.mm-file-download")).toHaveText("Télécharger");
    await expect(p.locator('.mm-editor [data-content-type="file"] .bn-file-name')).toHaveText("photo.png");
    await expect(images(p)).toHaveCount(0);
  }
  const address = (await tom.locator(".mm-editor a.mm-file-download").getAttribute("href"))!;
  expect(address).toMatch(FILE_URL);

  // Ouvrir son adresse dans l'onglet : un téléchargement, jamais un document.
  const here = tom.url();
  const [download] = await Promise.all([
    tom.waitForEvent("download"),
    tom.evaluate((url) => {
      window.location.href = url;
    }, address),
  ]);
  expect(download.suggestedFilename()).toBe("photo.png");
  expect(readFileSync(await download.path()).equals(HTML)).toBe(true);
  expect(tom.url()).toBe(here);
  // Dans un cadre, ou comme image : rien ne s'exécute non plus.
  const ran = await tom.evaluate(async (url) => {
    const frame = document.createElement("iframe");
    frame.src = url;
    const image = new Image();
    const loaded = new Promise<string>((done) => {
      image.onload = () => done("chargée");
      image.onerror = () => done("refusée");
    });
    image.src = url;
    document.body.append(frame, image);
    const outcome = await loaded;
    await new Promise((done) => setTimeout(done, 300));
    const inFrame = (() => {
      try {
        return (frame.contentWindow as (Window & { __ran?: boolean }) | null)?.__ran === true;
      } catch {
        return false;
      }
    })();
    frame.remove();
    image.remove();
    return { outcome, here: (window as Window & { __ran?: boolean }).__ran === true, inFrame };
  }, address);
  expect(ran).toEqual({ outcome: "refusée", here: false, inFrame: false });
  expect(await mona.evaluate(() => (window as Window & { __ran?: boolean }).__ran)).toBeUndefined();
  expect([...monaDialogs, ...tomDialogs]).toEqual([]);

  // Témoin : un vrai PNG, déposé de même, s'affiche chez les deux.
  await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name: "vraie.png", bytes: PNG, type: "image/png" });
  await expectShown(images(mona));
  await expectShown(images(tom));
});

test("image_is_no_longer_served_once_access_is_withdrawn", async ({ browser }) => {
  const { aliceApi, bruno, brunoId, page } = await sharedPrivate("read", "Retrait");
  const src = await upload(aliceApi, page.id, "photo.png", PNG);
  await putMmd(aliceApi, page.id, `![](${src}) {name="photo.png"}\n`);
  const tom = await openPage(browser, page.id, bruno);
  await expectShown(images(tom));
  /** Ce que l'onglet obtient de l'adresse : par `fetch`, et par une image neuve, cache compris. */
  const served = () =>
    tom.evaluate(async (url) => {
      const response = await fetch(url);
      const image = new Image();
      const shown = new Promise<boolean>((done) => {
        image.onload = () => done(true);
        image.onerror = () => done(false);
      });
      image.src = url;
      return { status: response.status, cache: response.headers.get("cache-control"), shown: await shown };
    }, src);
  // Témoin : tant que Tom lit la page, l'adresse le sert, et le navigateur ne la garde qu'à revalider.
  expect(await served()).toEqual({ status: 200, cache: "private, no-cache", shown: true });

  const withdrawn = await aliceApi.delete(`/api/v1/pages/${page.id}/access/users/${brunoId}`);
  expect(withdrawn.ok(), await withdrawn.text()).toBe(true);
  // La même adresse, depuis le même onglet, qui l'avait en cache : la page introuvable.
  expect(await served()).toEqual({ status: 404, cache: "no-store", shown: false });
  const body = await tom.evaluate(async (url) => (await fetch(url)).text(), src);
  expect(body).toContain("not_found");
  expect(body).not.toContain("PNG");
});

test("too_large_file_is_refused_with_the_limit", async ({ browser }) => {
  const { mona, aliceApi, page } = await duo(browser, "Trop gros");
  const sent = watchUploads(mona);
  const limits: string[] = [];
  mona.on("request", (request) => {
    if (request.url().endsWith("/api/v1/files/limits")) limits.push(request.url());
  });
  // Rien n'a été lu des limites à l'ouverture de la page.
  expect(limits).toEqual([]);
  await drop(mona, firstBlock(mona), {
    name: "gros.bin",
    bytes: Buffer.alloc(2 * 1024 * 1024, 7),
    type: "application/octet-stream",
  });
  await expect(refusals(mona)).toHaveText([/gros\.bin.* est trop gros : 1 Mio au plus par fichier sur cette instance/]);
  // Refusé avant toute requête d'envoi ; les limites, elles, ont été lues, une fois.
  expect(sent).toEqual([]);
  expect(limits).toHaveLength(1);
  await expect(uploads(mona)).toHaveCount(0);
  // Le bloc du dépôt reste, vide ; le nom du fichier, lui, n'est plus dans le document (R685).
  await expect(mona.locator(".mm-editor .bn-add-file-button")).toHaveCount(1);
  await expect.poll(() => getMmd(aliceApi, page.id)).not.toContain("gros.bin");

  // « Fermer » : le message part, et le focus revient au texte de la page.
  await mona.getByRole("button", { name: "Fermer", exact: true }).click();
  await expect(refusals(mona)).toHaveCount(0);
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();

  // Témoin : un fichier sous la limite part, et les limites ne sont pas relues.
  await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name: "photo.png", bytes: PNG, type: "image/png" });
  await expectShown(images(mona));
  expect(sent).toHaveLength(1);
  expect(limits).toHaveLength(1);
});

test("undo_and_restore_bring_the_image_back", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Retour");
  await drop(mona, firstBlock(mona), { name: "photo.png", bytes: PNG, type: "image/png" });
  await expectShown(images(mona));
  await expectShown(images(tom));
  const src = await images(mona).getAttribute("src");

  // Supprimée, puis annulée : elle revient, et s'affiche (le fichier n'a pas bougé).
  await images(mona).click();
  await mona.keyboard.press("Delete");
  await expect(images(mona)).toHaveCount(0);
  await expect(images(tom)).toHaveCount(0);
  await mona.keyboard.press("ControlOrMeta+z");
  await expectShown(images(mona));
  await expectShown(images(tom));
  expect(await images(tom).getAttribute("src")).toBe(src);

  // Supprimée de nouveau, plus d'un intervalle de versions après : une version garde la page d'avant.
  const before = (await versionsOf(aliceApi, page.id)).length;
  await mona.waitForTimeout(1_300);
  await images(mona).click();
  await mona.keyboard.press("Delete");
  await expect(images(tom)).toHaveCount(0);
  await expect.poll(async () => (await versionsOf(aliceApi, page.id)).length).toBeGreaterThan(before);
  const [version] = await versionsOf(aliceApi, page.id);
  const restored = await aliceApi.post(`/api/v1/pages/${page.id}/versions/${version.id}/restore`);
  expect(restored.status(), await restored.text()).toBe(204);
  await expectShown(images(mona));
  await expectShown(images(tom));
  expect(await images(tom).getAttribute("src")).toBe(src);
});

test("video_plays_and_seeks", async ({ browser }) => {
  const { mona, tom } = await duo(browser, "Vidéo");
  const ranges: string[] = [];
  tom.on("response", (response) => {
    if (/\/files\/[0-9a-f-]{36}$/.test(response.url()) && response.status() === 206) {
      ranges.push(response.headers()["content-range"] ?? "");
    }
  });
  await drop(mona, firstBlock(mona), { name: "demo.webm", bytes: VIDEO, type: "video/webm" });
  const video = tom.locator(".mm-editor video.bn-visual-media");
  await expect(video).toBeVisible({ timeout: 15_000 });
  expect(await video.getAttribute("src")).toMatch(FILE_URL);

  // Elle joue (sans le son : un onglet ne joue de lui-même qu'une vidéo muette).
  const played = await video.evaluate(async (v: HTMLVideoElement) => {
    v.muted = true;
    await v.play();
    await new Promise<void>((done) => {
      const check = () => (v.currentTime > 0.3 ? done() : requestAnimationFrame(check));
      check();
    });
    return { time: v.currentTime, duration: v.duration, width: v.videoWidth };
  });
  expect(played.time).toBeGreaterThan(0.3);
  expect(played.duration).toBeGreaterThan(9);
  expect(played.width).toBe(320);

  // On s'y déplace, et elle joue de là.
  const sought = await video.evaluate(async (v: HTMLVideoElement) => {
    const done = new Promise((resolve) => v.addEventListener("seeked", resolve, { once: true }));
    v.currentTime = 8;
    await done;
    await new Promise<void>((resolve) => {
      const check = () => (v.currentTime > 8.3 ? resolve() : requestAnimationFrame(check));
      check();
    });
    return { time: v.currentTime, error: v.error?.message ?? null };
  });
  expect(sought.error).toBeNull();
  expect(sought.time).toBeGreaterThan(8.3);
  // Le navigateur l'a demandée par plages (`206`), que le serveur sert.
  expect(ranges.length).toBeGreaterThan(0);
  expect(ranges.every((range) => /^bytes \d+-\d+\/\d+$/.test(range))).toBe(true);
});

test("an_upload_refused_to_the_account_is_said_as_the_server_says_it_and_the_page_follows", async ({ browser }) => {
  // Tom peut modifier la page, puis ne le peut plus, sans que son onglet l'ait appris : ni le canal de
  // l'espace, ni la fermeture de sa synchro ne le lui disent (R442). Le refus de son envoi le lui dit.
  const { aliceApi, bruno, brunoId, page } = await sharedPrivate("edit", "Refus");
  const tom = await newUser(browser, bruno);
  await closeEventsChannel(tom);
  await hideSyncClosures(tom);
  await tom.goto(`/p/${page.id}`);
  await waitSynced(tom);
  await expect(tom.locator('.mm-editor[data-editable="true"]')).toBeVisible();
  await setPageLevel(aliceApi, page.id, brunoId, "read");

  // Un fichier de près d'un Mio : le serveur répond une fois le corps reçu, et le refus arrive (R505).
  await drop(tom, firstBlock(tom), { name: "demo.webm", bytes: VIDEO, type: "video/webm" });
  await expect(refusals(tom)).toHaveText([/demo\.webm.* n'a pas été envoyé : .*Peut modifier/]);
  await expect(uploads(tom)).toHaveCount(0);
  // La page relue : elle passe en lecture, et le refus reste lisible.
  await expect(readerBanner(tom)).toBeVisible();
  await expect(refusals(tom)).toHaveCount(1);
  // Rien n'a été attaché à la page.
  expect(await getMmd(aliceApi, page.id)).not.toContain("/files/");
});

test("an_upload_in_course_is_shown_cancelled_and_leaves_with_the_page", async ({ browser }) => {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Envoi ${unique()}` });
  const other = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Ailleurs ${unique()}` });
  const mona = await newUser(browser, team.alice);
  // Les envois de l'onglet n'aboutissent que quand le test le veut.
  const held: (() => Promise<void>)[] = [];
  const failed: string[] = [];
  mona.on("requestfailed", (request) => {
    if (request.method() === "POST" && request.url().includes("/files?")) failed.push(request.failure()?.errorText ?? "");
  });
  await mona.route(/\/api\/v1\/pages\/[^/]+\/files\?/, (route) => {
    held.push(() => route.continue());
  });
  await mona.goto(`/p/${page.id}`);
  await waitSynced(mona);

  // Deux dépôts, l'un après l'autre sans attendre (les fichiers d'un même dépôt partent, eux, l'un
  // après l'autre : BlockNote attend chacun).
  await drop(mona, firstBlock(mona), { name: "un.png", bytes: PNG, type: "image/png" });
  await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name: "deux.png", bytes: PNG, type: "image/png" });
  // Deux envois de front, chacun dit, chacun avec de quoi l'annuler ; le bloc dit « Envoi… ».
  // Le plus récent en haut.
  await expect(uploads(mona)).toHaveText([/Envoi de deux\.png…/, /Envoi de un\.png…/]);
  await expect.poll(() => held.length).toBe(2);
  // Limite écrite (R685) : pendant l'envoi, le bloc en attente porte le nom du fichier, lu par tous.
  await expect.poll(() => getMmd(team.aliceApi, page.id)).toContain('name="un.png"');
  const waiting = mona.locator(".mm-editor .bn-file-loading-preview").first();
  await expect(waiting).toBeVisible();
  expect(await waiting.evaluate((e) => getComputedStyle(e, "::after").content)).toBe('"Envoi…"');

  // « Annuler » le premier : sa requête est coupée, rien n'est dit, le second continue.
  await mona.getByRole("button", { name: /^Annuler l'envoi de .*un\.png/ }).click();
  // Sa ligne reste, sans bouton, tant que le pointeur est dans la boîte ; puis elle part.
  await expect(uploads(mona)).toHaveText([/Envoi de deux\.png…/, /un\.png n'est plus en cours/]);
  await mona.mouse.move(20, 300);
  await expect(uploads(mona)).toHaveText([/Envoi de deux\.png…/]);
  // Le bloc que le dépôt avait posé reste, vide ; le nom du fichier n'est plus dans le document (R685).
  await expect(mona.locator(".mm-editor .bn-add-file-button")).toHaveCount(1);
  await expect.poll(() => getMmd(team.aliceApi, page.id)).not.toContain("un.png");
  await expect.poll(() => failed.length).toBe(1);
  await expect(refusals(mona)).toHaveCount(0);
  // Le focus n'est pas tombé sur `body`.
  expect(await mona.evaluate(() => document.activeElement === document.body)).toBe(false);

  // Le second aboutit : son image s'affiche, et plus aucun envoi n'est en cours.
  await held[1]();
  await expectShown(images(mona));
  await expect(uploads(mona)).toHaveCount(0);

  // Un troisième, en route quand on quitte la page : coupé, et la page suivante n'en dit rien.
  await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name: "trois.png", bytes: PNG, type: "image/png" });
  await expect(uploads(mona)).toHaveText([/Envoi de trois\.png…/]);
  await treeRow(mona, other.id).click();
  await waitSynced(mona);
  await expect.poll(() => failed.length).toBe(2);
  await expect(uploads(mona)).toHaveCount(0);
  await expect(refusals(mona)).toHaveCount(0);
  // Rien n'a été gardé dans le navigateur.
  const stored = await mona.evaluate(() => JSON.stringify([Object.keys(localStorage), Object.keys(sessionStorage)]));
  expect(stored).not.toMatch(/png|files/);
  // Une seule image est attachée à la première page.
  expect((await getMmd(await apiAs(team.alice), page.id)).match(/\/files\//g)).toHaveLength(1);
});

// Tour de correction 1 (relecture de C8).

test("a_copied_file_block_puts_only_a_permitted_address_in_the_clipboard", async ({ browser }) => {
  // Un bloc image à l'aperçu masqué, dont l'adresse n'est pas permise, écrit par MMD (un agent, un
  // autre client) : copié, il ne met dans le presse-papiers ni `href` ni `src` vers elle (I1).
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Copie ${unique()}` });
  const src = await upload(team.aliceApi, page.id, "bonne.png", PNG);
  await putMmd(
    team.aliceApi,
    page.id,
    `![]() {name="a.png" preview=false url="javascript:alert(document.domain)"}\n\n![](${src}) {name="bonne.png" preview=false}\n`,
  );
  const mona = await openPage(browser, page.id, team.alice);
  /** Ce que la copie du bloc cliqué met dans le presse-papiers : ses liens et ses sources. */
  const copied = async (name: string) => {
    await mona.locator(".mm-editor .bn-file-name", { hasText: name }).click();
    return mona.evaluate(() => {
      const data = new DataTransfer();
      document
        .querySelector(".mm-editor .bn-editor")!
        .dispatchEvent(new ClipboardEvent("copy", { clipboardData: data, bubbles: true, cancelable: true }));
      const host = document.createElement("div");
      host.innerHTML = data.getData("text/html");
      return {
        carried: [...host.querySelectorAll("[href], [src]")].map((e) => e.getAttribute("href") ?? e.getAttribute("src")),
        text: host.textContent ?? "",
      };
    });
  };
  const bad = await copied("a.png");
  expect(bad.carried).toEqual([]);
  expect(bad.text).toContain("a.png");
  // Témoin : un fichier de la page est copié avec son adresse.
  expect((await copied("bonne.png")).carried).toEqual([src]);
});

test("download_in_the_toolbar_does_not_hand_the_tab_to_the_site_it_opens", async ({ browser }) => {
  // Un bloc fichier à une adresse web, dont la page renvoie son ouvreur chez elle (I2).
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Ouvreur ${unique()}` });
  await putMmd(team.aliceApi, page.id, '<file src="https://ailleurs.example/rapport.pdf" name="rapport.pdf"/>\n');
  const mona = await newUser(browser, team.alice);
  await mona.context().route("https://ailleurs.example/**", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: route.request().url().includes("ailleurs.example/pris")
        ? "<title>pris</title>"
        : "<script>window.__had = !!window.opener; if (window.opener) window.opener.location = 'https://ailleurs.example/pris';</script>",
    }),
  );
  await mona.goto(`/p/${page.id}`);
  await waitSynced(mona);
  const here = mona.url();
  /** Ouvre le site par `click`, et dit s'il a eu un ouvreur et où est resté l'onglet de Mementum. */
  const opened = async (click: () => Promise<void>) => {
    const [site] = await Promise.all([mona.context().waitForEvent("page"), click()]);
    await site.waitForLoadState();
    const had = await site.evaluate(() => (window as Window & { __had?: boolean }).__had);
    // Le site a eu le temps d'agir : son script est passé (`__had` est posé).
    expect(had).toBeDefined();
    await site.close();
    return { had, tab: mona.url() };
  };
  // Par le lien du bloc, puis par « Télécharger » de la barre de mise en forme : jamais d'ouvreur.
  const link = mona.locator(".mm-editor a.mm-file-download");
  await expect(link).toHaveText("Ouvrir");
  expect(await opened(() => link.click())).toEqual({ had: false, tab: here });
  await mona.locator(".mm-editor .bn-file-name").click();
  const button = mona.getByRole("button", { name: /^Télécharger/ });
  await expect(button).toBeVisible();
  expect(await opened(() => button.click())).toEqual({ had: false, tab: here });
});

test("an_address_that_is_not_permitted_is_never_loaded", async ({ browser }) => {
  // Une adresse du document qui n'est ni un fichier de l'instance ni une adresse web : une autre route
  // de l'API. L'éditeur ne la met dans aucun `src`, et l'onglet ne la demande jamais.
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Refusée ${unique()}` });
  const src = await upload(team.aliceApi, page.id, "bonne.png", PNG);
  const forged = "/api/v1/files/limits?forgee";
  await putMmd(team.aliceApi, page.id, `![](${forged})\n\n![](${src}) {name="bonne.png"}\n\n<video src="${forged}"/>\n`);
  const mona = await newUser(browser, team.alice);
  const asked: string[] = [];
  mona.on("request", (request) => asked.push(new URL(request.url()).pathname + new URL(request.url()).search));
  await mona.goto(`/p/${page.id}`);
  await waitSynced(mona);
  // Barrière et témoin : l'image permise est demandée et affichée.
  await expectShown(mona.locator(`.mm-editor img[src="${src}"]`));
  expect(asked).toContain(src);
  await expect(images(mona)).toHaveCount(2);
  expect(await images(mona).first().getAttribute("src")).toBe("about:blank");
  expect(await mona.locator(".mm-editor video.bn-visual-media").getAttribute("src")).toBe("about:blank");
  expect(asked.filter((url) => url.includes("forgee"))).toEqual([]);
});

test("the_answer_of_the_server_is_set_in_the_block_only_if_it_is_a_file_of_the_page", async ({ browser }) => {
  // Un mandataire, ou un serveur d'une autre version, répond autre chose à l'envoi : rien n'est posé.
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Réponse ${unique()}` });
  const other = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Autre ${unique()}` });
  const elsewhere = await upload(team.aliceApi, other.id, "ailleurs.png", PNG);
  const mona = await newUser(browser, team.alice);
  const answers = [elsewhere, "/api/v1/files/limits?forgee", "javascript:alert(1)"];
  await mona.route(/\/api\/v1\/pages\/[^/]+\/files\?/, (route) =>
    route.fulfill({
      status: 201,
      contentType: "application/json",
      body: JSON.stringify({ id: "x", url: answers.shift(), name: "photo.png", size: PNG.length, mime: "image/png" }),
    }),
  );
  await mona.goto(`/p/${page.id}`);
  await waitSynced(mona);
  for (let n = 1; n <= 3; n++) {
    await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name: "photo.png", bytes: PNG, type: "image/png" });
    await expect(refusals(mona)).toHaveCount(n);
  }
  await expect(refusals(mona)).toHaveText([
    /photo\.png.* n'a pas été envoyé : le serveur a répondu autre chose qu'un fichier de cette page\./,
    /autre chose qu'un fichier de cette page/,
    /autre chose qu'un fichier de cette page/,
  ]);
  await expect(images(mona)).toHaveCount(0);
  // Rien de ces adresses dans le document, ni le nom du fichier : les blocs des dépôts restent, vides.
  await expect(mona.locator(".mm-editor .bn-add-file-button")).toHaveCount(3);
  await expect.poll(() => getMmd(team.aliceApi, page.id)).not.toMatch(/files|forgee|javascript|photo\.png/);
});

test("nothing_moves_under_the_pointer_in_the_box_of_uploads_and_it_covers_neither_the_title_nor_the_text", async ({
  browser,
}) => {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Pointeur ${unique()}` });
  await putMmd(team.aliceApi, page.id, "Première ligne de la page.\n\nSeconde ligne.\n");
  const mona = await newUser(browser, team.alice);
  const errors: string[] = [];
  mona.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  const held: (() => Promise<void>)[] = [];
  await mona.route(/\/api\/v1\/pages\/[^/]+\/files\?/, (route) => {
    held.push(() => route.continue());
  });
  await mona.goto(`/p/${page.id}`);
  await waitSynced(mona);
  for (const name of ["un.png", "deux.png", "trois.png"]) {
    await drop(mona, mona.locator(".mm-editor .bn-block-content").last(), { name, bytes: PNG, type: "image/png" });
  }
  await expect(uploads(mona)).toHaveCount(3);
  await expect.poll(() => held.length).toBe(3);

  // La boîte ne recouvre ni le titre, ni le texte, ni les blocs en attente (R675, R683).
  const covered = await mona.evaluate(() => {
    const box = document.querySelector(".mm-file-notices")!.getBoundingClientRect();
    const hit = (element: Element) => {
      const r = element.getBoundingClientRect();
      return r.left < box.right && box.left < r.right && r.top < box.bottom && box.top < r.bottom;
    };
    return [...document.querySelectorAll(".mm-title, .mm-topbar, .mm-editor .bn-block-content")].filter(hit).length;
  });
  expect(covered).toBe(0);

  // Le pointeur, immobile, sur « Annuler » du deuxième envoi ; le premier finit.
  const second = mona.getByRole("button", { name: /^Annuler l'envoi de .*deux\.png/ });
  await second.hover();
  const box = (await second.boundingBox())!;
  const at = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  const under = () =>
    mona.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.getAttribute("aria-label") ?? "(rien)", at);
  expect(await under()).toMatch(/deux\.png/);
  await held[0]();
  await expectShown(images(mona));
  // Sa ligne reste, sans bouton, et sous le pointeur c'est toujours le même bouton.
  await expect(uploads(mona)).toHaveText([/Envoi de trois\.png…/, /Envoi de deux\.png…/, /un\.png n'est plus en cours/]);
  expect(await under()).toMatch(/deux\.png/);
  expect(await second.boundingBox()).toEqual(box);
  // Le pointeur sorti de la boîte, la ligne finie part.
  await mona.mouse.move(20, 300);
  await expect(uploads(mona)).toHaveText([/Envoi de trois\.png…/, /Envoi de deux\.png…/]);
  // Témoin de la mutation : sans le pointeur, une ligne finie part tout de suite.
  await held[1]();
  await expect(uploads(mona)).toHaveText([/Envoi de trois\.png…/]);
  await held[2]();
  await expect(uploads(mona)).toHaveCount(0);
  // Un bloc en attente de son fichier ne charge rien : aucune erreur de console (m4).
  expect(errors).toEqual([]);
});

test("a_right_withdrawn_during_an_upload_is_said_without_inviting_to_try_again", async ({ browser }) => {
  // Le droit de Tom baisse pendant son envoi : son onglet l'apprend (sa synchro est fermée), sa session
  // est recréée en lecture, et l'envoi s'arrête. Le message dit ce que la page est devenue (m6).
  const { aliceApi, bruno, brunoId, page } = await sharedPrivate("edit", "Baisse");
  const tom = await newUser(browser, bruno);
  await tom.route(/\/api\/v1\/pages\/[^/]+\/files\?/, () => {});
  await tom.goto(`/p/${page.id}`);
  await waitSynced(tom);
  await drop(tom, firstBlock(tom), { name: "photo.png", bytes: PNG, type: "image/png" });
  await expect(uploads(tom)).toHaveCount(1);
  await setPageLevel(aliceApi, page.id, brunoId, "read");
  await expect(readerBanner(tom)).toBeVisible({ timeout: 15_000 });
  await expect(refusals(tom)).toHaveText([/photo\.png.* n'a pas été envoyé : tu ne peux plus modifier cette page\./]);
  await expect(uploads(tom)).toHaveCount(0);
  expect((await refusals(tom).allTextContents()).join(" ")).not.toContain("Réessaie");
});

// Un fichier venu d'une autre page (Task C9, R249) : un bloc collé garde l'adresse de sa première
// page ; l'éditeur fait recopier l'attache dans la page où l'on colle, puis récrit l'adresse du bloc.

/** Ce que la copie du bloc de fichier cliqué met dans le presse-papiers, par type. */
async function copyClicked(p: Page, target: Locator): Promise<Record<string, string>> {
  await target.click();
  return p.evaluate(() => {
    const data = new DataTransfer();
    document
      .querySelector(".mm-editor .bn-editor")!
      .dispatchEvent(new ClipboardEvent("copy", { clipboardData: data, bubbles: true, cancelable: true }));
    return Object.fromEntries([...data.types].map((type) => [type, data.getData(type)]));
  });
}

/** Colle `data` (par type) là où est le curseur, comme le presse-papiers. */
async function pasteData(p: Page, data: Record<string, string>): Promise<void> {
  await p.locator(".mm-editor .bn-editor").evaluate((editor, given) => {
    const clipboardData = new DataTransfer();
    for (const [type, value] of Object.entries(given)) clipboardData.setData(type, value);
    editor.dispatchEvent(new ClipboardEvent("paste", { clipboardData, bubbles: true, cancelable: true }));
  }, data);
}

/** Les recopies d'attache parties de l'onglet. */
function watchCopies(p: Page): string[] {
  const sent: string[] = [];
  p.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/files/copies")) sent.push(request.postData() ?? "");
  });
  return sent;
}

/** Une page privée de Mona qui porte une image, et une page d'équipe, vide, que Tom modifie aussi. */
async function twoPages() {
  const team = await ownerAndMember();
  const first = await createPage(team.aliceApi, {
    spaceId: team.space.id,
    section: "private",
    title: `Privée ${unique()}`,
  });
  const src = await upload(team.aliceApi, first.id, "photo.png", PNG);
  await putMmd(team.aliceApi, first.id, `![](${src}) {name="photo.png"}\n`);
  const shared = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Équipe ${unique()}` });
  return { ...team, first, src, shared };
}

test("pasted_image_is_seen_by_someone_who_cannot_read_its_first_page", async ({ browser }) => {
  const { alice, bruno, aliceApi, first, src, shared } = await twoPages();
  // Mona copie l'image de sa page privée.
  const source = await openPage(browser, first.id, alice);
  await expectShown(images(source));
  const clipboard = await copyClicked(source, images(source));
  expect(Object.values(clipboard).join("\n")).toContain(src);

  // Elle la colle dans la page d'équipe ; la recopie est retenue le temps du témoin.
  const mona = await newUser(browser, alice);
  const copies = watchCopies(mona);
  let release!: () => void;
  const held = new Promise<void>((done) => (release = done));
  await mona.route(/\/files\/copies$/, async (route) => {
    await held;
    await route.continue();
  });
  await mona.goto(`/p/${shared.id}`);
  await waitSynced(mona);
  // Rien à l'ouverture de la page.
  expect(copies).toEqual([]);
  await firstBlock(mona).click();
  await pasteData(mona, clipboard);
  await expectShown(images(mona));
  await expect.poll(() => copies.length).toBe(1);
  expect(JSON.parse(copies[0])).toEqual({ pageId: first.id, fileId: src.split("/").pop() });

  // Témoin (preuve 2) : tant que le bloc nomme le fichier de la page privée, Tom, qui ne la voit pas,
  // ne reçoit rien : c'est le serveur qui décide.
  const tom = await openPage(browser, shared.id, bruno);
  await expect(images(tom)).toHaveCount(1);
  expect(await images(tom).getAttribute("src")).toBe(src);
  expect(await tom.evaluate(async (url) => (await fetch(url)).status, src)).toBe(404);
  await expect
    .poll(() => images(tom).evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth === 0))
    .toBe(true);

  // La recopie aboutit : l'adresse du bloc devient celle d'un fichier de la page d'équipe, et Tom voit l'image.
  release();
  await expect.poll(() => images(tom).getAttribute("src")).toContain(`/pages/${shared.id}/files/`);
  await expectShown(images(tom));
  await expectShown(images(mona));
  expect(await images(mona).getAttribute("src")).toBe(await images(tom).getAttribute("src"));
  expect(copies).toHaveLength(1);
  await expect(refusals(mona)).toHaveCount(0);
  // Le document ne nomme plus la page privée ; la page privée, elle, n'a pas changé.
  const mmd = await getMmd(aliceApi, shared.id);
  expect(mmd).toContain(`/api/v1/pages/${shared.id}/files/`);
  expect(mmd).not.toContain(first.id);
  expect(await getMmd(aliceApi, first.id)).toBe(`![](${src}) {name="photo.png"}\n`);
  // Tom ne lit toujours pas le fichier de la page privée.
  expect(await tom.evaluate(async (url) => (await fetch(url)).status, src)).toBe(404);
});

test("pasted_image_survives_the_deletion_of_its_first_page", async ({ browser }) => {
  const { alice, bruno, aliceApi, first, shared } = await twoPages();
  const source = await openPage(browser, first.id, alice);
  await expectShown(images(source));
  const clipboard = await copyClicked(source, images(source));
  const mona = await openPage(browser, shared.id, alice);
  await firstBlock(mona).click();
  await pasteData(mona, clipboard);
  await expect.poll(() => images(mona).getAttribute("src")).toContain(`/pages/${shared.id}/files/`);
  const here = (await images(mona).getAttribute("src"))!;

  // La première page est supprimée pour de bon.
  await source.close();
  expect((await aliceApi.post(`/api/v1/pages/${first.id}/trash`)).ok()).toBe(true);
  expect((await aliceApi.delete(`/api/v1/pages/${first.id}`)).status()).toBe(204);

  // Le fichier de la page d'équipe est toujours servi, à une page rouverte, pour Tom aussi.
  const tom = await openPage(browser, shared.id, bruno);
  await expectShown(images(tom));
  expect(await images(tom).getAttribute("src")).toBe(here);
  const served = await tom.evaluate(async (url) => {
    const response = await fetch(url);
    return { status: response.status, bytes: (await response.arrayBuffer()).byteLength };
  }, here);
  expect(served).toEqual({ status: 200, bytes: PNG.length });
});

test("a_pasted_file_of_a_page_one_cannot_read_stays_as_it_is_and_is_never_served", async ({ browser }) => {
  // Tom modifie la page d'équipe, et ne voit pas la page privée de Mona. Il colle un bloc qui nomme le
  // fichier de cette page (le presse-papiers lui vient d'ailleurs) : la recopie est refusée, d'une seule
  // réponse, et son bloc garde l'adresse, que le serveur ne lui sert pas.
  const { alice, bruno, aliceApi, first, src, shared } = await twoPages();
  const source = await openPage(browser, first.id, alice);
  await expectShown(images(source));
  const clipboard = await copyClicked(source, images(source));
  const tom = await newUser(browser, bruno);
  const copies = watchCopies(tom);
  const answers: number[] = [];
  tom.on("response", (response) => {
    if (response.url().endsWith("/files/copies")) answers.push(response.status());
  });
  await tom.goto(`/p/${shared.id}`);
  await waitSynced(tom);
  await firstBlock(tom).click();
  await pasteData(tom, clipboard);
  await expect.poll(() => answers).toEqual([422]);
  await expect(images(tom)).toHaveCount(1);
  expect(await images(tom).getAttribute("src")).toBe(src);
  await expect
    .poll(() => images(tom).evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth === 0))
    .toBe(true);
  expect(await tom.evaluate(async (url) => (await fetch(url)).status, src)).toBe(404);
  // Rien n'est dit de plus, et rien n'a été attaché à la page d'équipe.
  await expect(refusals(tom)).toHaveCount(0);
  expect(copies).toHaveLength(1);
  expect(await getMmd(aliceApi, shared.id)).toContain(src);
  expect(await getMmd(aliceApi, shared.id)).not.toContain(`/pages/${shared.id}/files/`);

  // Rouvrir la page, ou la lire sans la modifier, ne recopie rien : la recopie ne part qu'au collage.
  const again = await newUser(browser, alice);
  const reopened = watchCopies(again);
  await again.goto(`/p/${shared.id}`);
  await waitSynced(again);
  // Barrière : Mona, qui lit la page privée, voit l'image, par l'adresse de sa première page.
  await expectShown(images(again));
  expect(await images(again).getAttribute("src")).toBe(src);
  expect(reopened).toEqual([]);
});
