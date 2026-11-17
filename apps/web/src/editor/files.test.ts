// Les fichiers d'une page, côté client (Task C8) : quelles adresses l'éditeur charge, et ce que
// fait un envoi, du choix du fichier à ce qui est posé dans le bloc. Le serveur est simulé : `upload`
// est une fonction du test, qui répond quand il veut.
import type { FileInfo, FileLimits, PageNode } from "@mementum/protocol";
import { afterEach, expect, test } from "vitest";
import { ApiError } from "../api";
import { page } from "../test/pages";
import {
  BLANK,
  closeRefusals,
  enterFiles,
  fileLimits,
  filePath,
  filesOf,
  forgetFileLimits,
  isFilePath,
  leaveFiles,
  sayRefusal,
  shownUrl,
  uploader,
  watchFiles,
} from "./files";

const PAGE = "0192f0c1-0000-7000-8000-000000000001";
const OTHER = "0192f0c1-0000-7000-8000-000000000002";
const FILE = "0192f0c1-0000-7000-8000-0000000000f1";
const PATH = `/api/v1/pages/${PAGE}/files/${FILE}`;
const MIB = 1024 * 1024;

afterEach(() => {
  leaveFiles(PAGE);
  leaveFiles(OTHER);
  forgetFileLimits();
});

test("filePath_takes_only_the_exact_path_of_a_file_of_the_instance", () => {
  expect(filePath(PATH)).toEqual({ pageId: PAGE, fileId: FILE });
  expect(isFilePath(PATH)).toBe(true);
  const refused: unknown[] = [
    `http://localhost${PATH}`,
    `https://ailleurs.example${PATH}`,
    `//ailleurs.example${PATH}`,
    `${PATH}?x`,
    `${PATH}?`,
    `${PATH}#x`,
    `${PATH}/`,
    `${PATH}/../${FILE}`,
    `/api/v1/pages/${PAGE}/../${PAGE}/files/${FILE}`,
    ` ${PATH}`,
    `${PATH} `,
    `${PATH}\n`,
    `\t${PATH}`,
    PATH.slice(1),
    `.${PATH}`,
    `\\api\\v1\\pages\\${PAGE}\\files\\${FILE}`,
    "/api/v1/pages/x/files/y",
    `/api/v1/pages/${PAGE}/files/y`,
    `/api/v1/pages/x/files/${FILE}`,
    `/api/v1/pages/${PAGE}/files/copies`,
    `/api/v1/pages/${PAGE}/files`,
    `/api/v1/pages/${PAGE}/versions/${FILE}`,
    `/api/v2/pages/${PAGE}/files/${FILE}`,
    `/API/v1/pages/${PAGE}/files/${FILE}`,
    `/api/v1/pages/${PAGE}/files/${FILE}%2F..`,
    `/api/v1/pages/%30192f0c1-0000-7000-8000-000000000001/files/${FILE}`,
    "",
    "javascript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "blob:http://localhost/1",
    null,
    undefined,
    7,
    { toString: () => PATH },
    [PATH],
  ];
  for (const url of refused) {
    expect(filePath(url), String(url)).toBeNull();
    expect(isFilePath(url), String(url)).toBe(false);
  }
});

test("shownUrl_gives_a_file_path_or_a_web_address_and_a_blank_page_for_anything_else", () => {
  expect(shownUrl(PATH)).toBe(PATH);
  expect(shownUrl("https://exemple.fr/a.png")).toBe("https://exemple.fr/a.png");
  expect(shownUrl("http://exemple.fr/a.png?x=1#y")).toBe("http://exemple.fr/a.png?x=1#y");
  // Une adresse web est rendue telle que le navigateur l'écrit.
  expect(shownUrl(" https://exemple.fr ")).toBe("https://exemple.fr/");
  const refused: unknown[] = [
    "javascript:alert(1)",
    "JaVaScRiPt:alert(1)",
    " javascript:alert(1)",
    "java\nscript:alert(1)",
    "data:image/svg+xml,<svg onload=alert(1)>",
    "blob:http://localhost/1",
    "file:///etc/passwd",
    "ftp://exemple.fr/a",
    "about:blank",
    "//exemple.fr/a.png",
    "/api/v1/auth/logout",
    "/api/v1/pages/x/files/y",
    `${PATH}?x`,
    `${PATH}/../${FILE}`,
    `http://localhost${PATH}x`.replace("http://", "http:"),
    "exemple.fr/a.png",
    "a.png",
    "../a.png",
    "",
    null,
    undefined,
    { href: "https://exemple.fr" },
  ];
  for (const url of refused) expect(shownUrl(url), String(url)).toBe(BLANK);
  expect(BLANK).toBe("about:blank");
});

// Le banc d'un envoi : un serveur qui répond quand le test le veut.

type Sent = {
  pageId: string;
  file: File;
  signal: AbortSignal;
  answer(info: Partial<FileInfo>): void;
  fail(error: unknown): void;
};

function bench({
  max = MIB,
  types = {} as Record<string, string>,
  reread = async (): Promise<PageNode | null | undefined> => page(PAGE),
} = {}) {
  const sent: Sent[] = [];
  const refused: string[] = [];
  /** Les blocs rendus par un envoi qui n'a pas abouti : `id:nom du fichier`. */
  const discarded: string[] = [];
  const state = { limitsRead: 0, rereads: 0, max };
  const controller = new AbortController();
  const upload = uploader({
    pageId: PAGE,
    limits: () =>
      fileLimits(async (): Promise<FileLimits> => {
        state.limitsRead += 1;
        return { maxFileBytes: state.max };
      }),
    blockType: (id) => types[id],
    onRefused: (message) => refused.push(message),
    reread: () => {
      state.rereads += 1;
      return reread();
    },
    discard: (id, name) => discarded.push(`${id}:${name}`),
    signal: controller.signal,
    upload: (pageId, file, signal) =>
      new Promise<FileInfo>((answer, fail) => {
        // Comme `fetch` : une requête annulée rejette.
        signal?.addEventListener("abort", () => fail(new DOMException("annulé", "AbortError")));
        sent.push({
          pageId,
          file,
          signal: signal!,
          answer: (info) =>
            answer({ id: FILE, url: PATH, name: file.name, size: file.size, mime: "image/png", ...info }),
          fail,
        });
      }),
  });
  return { upload, sent, refused, discarded, state, controller, types };
}

const png = (name = "photo.png", size = 3) => new File([new Uint8Array(size)], name, { type: "image/png" });

/** Attend `condition`, deux secondes au plus. */
async function until(condition: () => boolean, what: string): Promise<void> {
  const end = performance.now() + 2_000;
  while (!condition()) {
    if (performance.now() > end) throw new Error(`jamais vu : ${what}`);
    await new Promise((done) => setTimeout(done, 0));
  }
}

/** L'issue d'une promesse, sans la laisser rejeter sans gestionnaire. */
function settled<T>(promise: Promise<T>): Promise<{ value?: T; error?: unknown }> {
  return promise.then(
    (value) => ({ value }),
    (error: unknown) => ({ error }),
  );
}

test("an_upload_gives_the_address_and_the_name_the_server_says", async () => {
  const b = bench({ types: { b1: "image" } });
  const done = b.upload(png("  photo\u202E.png"), "b1");
  await until(() => b.sent.length === 1, "l'envoi");
  expect(b.sent[0].pageId).toBe(PAGE);
  // Le nom du bloc est celui que le serveur a nettoyé, pas celui du fichier.
  b.sent[0].answer({ name: "photo.png" });
  expect(await done).toEqual({ props: { url: PATH, name: "photo.png" } });
  expect(b.refused).toEqual([]);
});

test("a_file_that_is_not_an_image_becomes_a_file_block", async () => {
  const cases: [block: string, mime: string, expected: "same" | "file"][] = [
    ["image", "image/png", "same"],
    ["image", "application/octet-stream", "file"],
    ["image", "video/webm", "file"],
    ["image", "audio/mpeg", "file"],
    ["image", "application/pdf", "file"],
    ["image", "imagery/png", "file"],
    ["video", "video/webm", "same"],
    ["audio", "audio/mpeg", "same"],
    ["file", "image/png", "same"],
    ["file", "application/octet-stream", "same"],
  ];
  for (const [block, mime, expected] of cases) {
    const b = bench({ types: { b1: block } });
    const done = b.upload(png(), "b1");
    await until(() => b.sent.length === 1, "l'envoi");
    b.sent[0].answer({ mime });
    const props = { url: PATH, name: "photo.png" };
    expect(await done, `${block} ${mime}`).toEqual(expected === "same" ? { props } : { type: "file", props });
  }
});

test("a_file_larger_than_the_limit_is_refused_before_any_request", async () => {
  const b = bench({ max: 2 * MIB, types: { b1: "image" } });
  // Rien n'est lu avant le premier envoi.
  expect(b.state.limitsRead).toBe(0);
  const big = await settled(b.upload(png("gros \u2068fichier.png", 2 * MIB + 1), "b1"));
  expect(big.error).toBeDefined();
  expect(b.sent).toEqual([]);
  expect(b.refused).toEqual([
    "\u2068gros fichier.png\u2069 est trop gros : 2 Mio au plus par fichier sur cette instance. Rien n'a été envoyé.",
  ]);
  expect(filesOf(PAGE).uploads).toEqual([]);
  // À la limite juste, il part ; et les limites ne sont lues qu'une fois.
  const exact = b.upload(png("juste.png", 2 * MIB), "b1");
  await until(() => b.sent.length === 1, "l'envoi du fichier à la limite");
  b.sent[0].answer({});
  await exact;
  expect(b.state.limitsRead).toBe(1);
  expect(b.refused).toHaveLength(1);
});

test("limits_that_cannot_be_read_refuse_the_upload_and_are_read_again_at_the_next", async () => {
  let fail = true;
  const refused: string[] = [];
  const sent: File[] = [];
  const upload = uploader({
    pageId: PAGE,
    limits: () =>
      fileLimits(async () => {
        if (fail) throw new ApiError(0, null, "Le serveur Mementum ne répond pas.");
        return { maxFileBytes: MIB };
      }),
    blockType: () => "image",
    onRefused: (message) => refused.push(message),
    reread: async () => page(PAGE),
    signal: new AbortController().signal,
    upload: async (_page, file) => {
      sent.push(file);
      return { id: FILE, url: PATH, name: file.name, size: file.size, mime: "image/png" };
    },
  });
  expect((await settled(upload(png(), "b1"))).error).toBeDefined();
  expect(sent).toEqual([]);
  expect(refused).toEqual(["\u2068photo.png\u2069 n'a pas été envoyé : Le serveur Mementum ne répond pas."]);
  fail = false;
  expect(await upload(png(), "b1")).toEqual({ props: { url: PATH, name: "photo.png" } });
});

test("an_upload_refused_by_the_server_is_said_as_it_says_it", async () => {
  const refusals: [status: number, code: ApiError["code"], message: string][] = [
    [403, "forbidden", "il te faut « Peut modifier » sur cette page"],
    [409, "trashed", "la page est à la corbeille"],
    [413, "file_too_large", "fichier trop gros : 1 Mio au plus sur cette instance"],
    [404, "not_found", "page introuvable"],
  ];
  for (const [status, code, message] of refusals) {
    const b = bench({ types: { b1: "image" } });
    const done = settled(b.upload(png(), "b1"));
    await until(() => b.sent.length === 1, "l'envoi");
    b.sent[0].fail(new ApiError(status, code, message));
    expect((await done).error, String(status)).toBeInstanceOf(ApiError);
    expect(b.refused, String(status)).toEqual([`\u2068photo.png\u2069 n'a pas été envoyé : ${message}`]);
    // La page est relue : un droit, ou la page, ont changé.
    expect(b.state.rereads, String(status)).toBe(1);
    expect(filesOf(PAGE).uploads).toEqual([]);
  }
});

test("a_413_makes_the_next_upload_read_the_limits_again", async () => {
  const b = bench({ max: 2 * MIB, types: { b1: "image" } });
  const first = settled(b.upload(png("a.png", MIB + 1), "b1"));
  await until(() => b.sent.length === 1, "l'envoi");
  // La limite a baissé sur le serveur depuis sa lecture.
  b.state.max = MIB;
  b.sent[0].fail(new ApiError(413, "file_too_large", "fichier trop gros : 1 Mio au plus sur cette instance"));
  await first;
  expect((await settled(b.upload(png("a.png", MIB + 1), "b1"))).error).toBeDefined();
  expect(b.state.limitsRead).toBe(2);
  expect(b.sent).toHaveLength(1);
  expect(b.refused[1]).toContain("1 Mio au plus par fichier");
});

test("a_cut_upload_reads_the_page_again_and_says_what_it_has_become", async () => {
  const cases: [what: PageNode | null | undefined, message: string][] = [
    [undefined, "\u2068photo.png\u2069 n'a pas été envoyé : le serveur ne répond pas. Vérifie ta connexion, puis réessaie."],
    [null, "\u2068photo.png\u2069 n'a pas été envoyé : tu n'as plus accès à cette page."],
    [page(PAGE, { level: "comment" }), "\u2068photo.png\u2069 n'a pas été envoyé : tu ne peux plus modifier cette page."],
    [page(PAGE, { level: "read" }), "\u2068photo.png\u2069 n'a pas été envoyé : tu ne peux plus modifier cette page."],
    [page(PAGE, { trashedAt: 1 }), "\u2068photo.png\u2069 n'a pas été envoyé : la page est à la corbeille."],
    [page(PAGE), "L'envoi de \u2068photo.png\u2069 a été interrompu. Réessaie."],
  ];
  // Une coupure du réseau, ou un refus de session : le serveur a pu répondre sans lire le corps,
  // et le navigateur n'en voit qu'une coupure (R505).
  const cuts = [new ApiError(0, null, "Le serveur Mementum ne répond pas."), new ApiError(401, "unauthenticated", "session absente")];
  for (const cut of cuts) {
    for (const [what, message] of cases) {
      const b = bench({ types: { b1: "image" }, reread: async () => what });
      const done = settled(b.upload(png(), "b1"));
      await until(() => b.sent.length === 1, "l'envoi");
      b.sent[0].fail(cut);
      expect((await done).error).toBe(cut);
      await until(() => b.refused.length === 1, `le message (${message})`);
      expect(b.refused).toEqual([message]);
      expect(b.state.rereads).toBe(1);
    }
  }
});

test("an_upload_is_cancelled_when_the_page_closes", async () => {
  const b = bench({ types: { b1: "image" } });
  enterFiles(PAGE);
  const done = settled(b.upload(png(), "b1"));
  await until(() => b.sent.length === 1, "l'envoi");
  expect(filesOf(PAGE).uploads.map((u) => u.name)).toEqual(["photo.png"]);
  // La page est quittée : l'éditeur part (son signal), puis la page.
  b.controller.abort();
  leaveFiles(PAGE);
  expect(b.sent[0].signal.aborted).toBe(true);
  expect((await done).error).toBeDefined();
  // Plus rien après : ni message, ni relecture, ni envoi en cours, ni envoi neuf.
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(filesOf(PAGE)).toEqual({ uploads: [], refusals: [] });
  expect(b.state.rereads).toBe(0);
  // L'éditeur est parti : son bloc ne lui est pas rendu (il n'écrit plus pour personne).
  expect(b.discarded).toEqual([]);
  expect((await settled(b.upload(png(), "b1"))).error).toBeDefined();
  expect(b.sent).toHaveLength(1);
  // Une réponse arrivée quand même ne pose rien.
  b.sent[0].answer({});
  expect(filesOf(PAGE)).toEqual({ uploads: [], refusals: [] });
});

test("an_upload_cancelled_by_the_person_says_nothing_and_leaves_the_others", async () => {
  const b = bench({ types: { b1: "image", b2: "image" } });
  enterFiles(PAGE);
  const first = settled(b.upload(png("un.png"), "b1"));
  const second = b.upload(png("deux.png"), "b2");
  await until(() => b.sent.length === 2, "les deux envois");
  const shown = filesOf(PAGE).uploads;
  expect(shown.map((u) => u.name)).toEqual(["un.png", "deux.png"]);
  shown[0].cancel();
  expect((await first).error).toBeDefined();
  expect(b.sent[0].signal.aborted).toBe(true);
  expect(b.sent[1].signal.aborted).toBe(false);
  expect(filesOf(PAGE).uploads.map((u) => u.name)).toEqual(["deux.png"]);
  b.sent[1].answer({ name: "deux.png" });
  expect(await second).toEqual({ props: { url: PATH, name: "deux.png" } });
  expect(b.refused).toEqual([]);
  expect(b.state.rereads).toBe(0);
  expect(filesOf(PAGE).uploads).toEqual([]);
});

test("a_block_removed_during_the_upload_is_said_and_nothing_is_set", async () => {
  const b = bench({ types: { b1: "image" } });
  const done = settled(b.upload(png(), "b1"));
  await until(() => b.sent.length === 1, "l'envoi");
  delete b.types.b1;
  b.sent[0].answer({});
  expect((await done).error).toBeDefined();
  expect(b.refused).toEqual([
    "\u2068photo.png\u2069 a été envoyé, mais son bloc a été supprimé ou changé entre-temps : il n'est pas dans la page.",
  ]);
});

test("an_answer_that_is_not_a_file_of_this_page_sets_nothing", async () => {
  const answers: Partial<FileInfo>[] = [
    { url: `/api/v1/pages/${OTHER}/files/${FILE}` },
    { url: "javascript:alert(1)" },
    { url: `https://ailleurs.example${PATH}` },
    { url: 7 as unknown as string },
    { name: { toString: () => "x" } as unknown as string },
    { mime: 7 as unknown as string },
  ];
  for (const answer of answers) {
    const b = bench({ types: { b1: "image" } });
    const done = settled(b.upload(png(), "b1"));
    await until(() => b.sent.length === 1, "l'envoi");
    b.sent[0].answer(answer);
    expect((await done).error, JSON.stringify(answer)).toBeDefined();
    expect(b.refused).toEqual(["\u2068photo.png\u2069 n'a pas été envoyé : le serveur a répondu autre chose qu'un fichier de cette page."]);
  }
});

test("a_long_or_misleading_file_name_is_cut_and_isolated_in_a_message", async () => {
  const b = bench({ max: 1, types: { b1: "image" } });
  await settled(b.upload(png(`${"a".repeat(300)}\u202Egnp.exe\u2069\u2069`, 2), "b1"));
  const [message] = b.refused;
  // Isolé : ni marque de direction du nom, ni isolement fermé par lui.
  expect(message.startsWith(`\u2068${"a".repeat(80)}…\u2069 est trop gros`)).toBe(true);
  expect([...message].filter((c) => c === "\u2069")).toHaveLength(1);
  expect(message).not.toContain("\u202E");
});

test("the_messages_and_uploads_of_a_page_are_its_own_and_leave_with_it", () => {
  const seen: number[] = [];
  const stop = watchFiles(() => seen.push(filesOf(PAGE).refusals.length));
  // Une page qu'on ne regarde pas ne garde rien.
  sayRefusal(PAGE, "avant");
  expect(filesOf(PAGE).refusals).toEqual([]);
  enterFiles(PAGE);
  enterFiles(OTHER);
  sayRefusal(PAGE, "un");
  sayRefusal(PAGE, "deux");
  sayRefusal(OTHER, "ailleurs");
  expect(filesOf(PAGE).refusals.map((r) => r.text)).toEqual(["un", "deux"]);
  expect(filesOf(OTHER).refusals.map((r) => r.text)).toEqual(["ailleurs"]);
  // Le même objet tant que rien ne change (React le relit à chaque rendu).
  expect(filesOf(PAGE)).toBe(filesOf(PAGE));
  closeRefusals(PAGE);
  expect(filesOf(PAGE).refusals).toEqual([]);
  expect(filesOf(OTHER).refusals).toHaveLength(1);
  sayRefusal(PAGE, "trois");
  leaveFiles(PAGE);
  expect(filesOf(PAGE)).toEqual({ uploads: [], refusals: [] });
  sayRefusal(PAGE, "après");
  expect(filesOf(PAGE).refusals).toEqual([]);
  stop();
  expect(seen.length).toBeGreaterThanOrEqual(4);
});

test("nothing_of_a_file_is_kept_in_the_storage_of_the_browser", async () => {
  localStorage.clear();
  sessionStorage.clear();
  const b = bench({ types: { b1: "image" } });
  enterFiles(PAGE);
  const done = b.upload(png(), "b1");
  await until(() => b.sent.length === 1, "l'envoi");
  b.sent[0].answer({});
  await done;
  sayRefusal(PAGE, "un refus");
  expect(localStorage.length).toBe(0);
  expect(sessionStorage.length).toBe(0);
});

// Tour de correction 1 (relecture de C8).

/** Un nom tel qu'un message le cite : isolé (U+2068, U+2069). */
const cited = (name: string) => `${String.fromCharCode(0x2068)}${name}${String.fromCharCode(0x2069)}`;

test("a_block_that_is_no_longer_a_file_block_is_said_and_nothing_is_set", async () => {
  // Un autre a changé le bloc en paragraphe (ou en autre chose) pendant l'envoi : ce que l'envoi
  // rendrait n'y serait pas posé, et le fichier resterait attaché sans que rien le dise (m3).
  for (const type of ["paragraph", "heading", "callout", "bookmark"]) {
    const b = bench({ types: { b1: "image" } });
    const done = settled(b.upload(png(), "b1"));
    await until(() => b.sent.length === 1, "l'envoi");
    b.types.b1 = type;
    b.sent[0].answer({});
    expect((await done).error, type).toBeDefined();
    expect(b.refused, type).toEqual([
      `${cited("photo.png")} a été envoyé, mais son bloc a été supprimé ou changé entre-temps : il n'est pas dans la page.`,
    ]);
    // Le bloc n'est plus celui de l'envoi : il n'y touche pas.
    expect(b.discarded, type).toEqual([]);
  }
});

test("a_cut_or_a_server_error_makes_the_next_upload_read_the_limits_again", async () => {
  // Le `413` d'un fichier trop gros n'arrive pas à un navigateur (R505) : il n'en voit qu'une coupure,
  // ou l'erreur d'un mandataire. Une limite périmée serait sinon gardée sans fin (m5).
  const failures = [
    new ApiError(0, null, "Le serveur Mementum ne répond pas."),
    new ApiError(502, null, "Le serveur Mementum ne répond pas."),
    new ApiError(500, "internal", "erreur interne"),
    new TypeError("Failed to fetch"),
  ];
  for (const failure of failures) {
    forgetFileLimits();
    const b = bench({ max: 100 * MIB, types: { b1: "image" } });
    const first = settled(b.upload(png("gros.png", 2 * MIB), "b1"));
    await until(() => b.sent.length === 1, "l'envoi");
    b.state.max = MIB;
    b.sent[0].fail(failure);
    await first;
    await until(() => b.refused.length === 1, "le message de la coupure");
    // L'essai suivant relit la limite, et dit la vraie, sans rien envoyer.
    expect((await settled(b.upload(png("gros.png", 2 * MIB), "b1"))).error).toBeDefined();
    expect(b.state.limitsRead, String(failure)).toBe(2);
    expect(b.sent, String(failure)).toHaveLength(1);
    expect(b.refused[1], String(failure)).toContain("1 Mio au plus par fichier");
  }
  // Témoin : un refus fait au compte (`403`) garde la limite lue.
  forgetFileLimits();
  const b = bench({ types: { b1: "image" } });
  const refused = settled(b.upload(png(), "b1"));
  await until(() => b.sent.length === 1, "l'envoi");
  b.sent[0].fail(new ApiError(403, "forbidden", "il te faut « Peut modifier »"));
  await refused;
  const again = b.upload(png(), "b1");
  await until(() => b.sent.length === 2, "le second envoi");
  b.sent[1].answer({});
  await again;
  expect(b.state.limitsRead).toBe(1);
});

test("an_upload_stopped_because_the_session_of_the_page_is_made_again_says_what_the_page_has_become", async () => {
  // L'éditeur est remplacé pendant l'envoi (droits baissés, corbeille, schéma), la page reste
  // ouverte : c'est la page relue qui dit pourquoi, jamais « Réessaie » à qui ne peut plus écrire (m6).
  const cases: [what: PageNode | null | undefined, message: string][] = [
    [page(PAGE, { level: "read" }), `${cited("photo.png")} n'a pas été envoyé : tu ne peux plus modifier cette page.`],
    [page(PAGE, { trashedAt: 1 }), `${cited("photo.png")} n'a pas été envoyé : la page est à la corbeille.`],
    [null, `${cited("photo.png")} n'a pas été envoyé : tu n'as plus accès à cette page.`],
    [page(PAGE), `L'envoi de ${cited("photo.png")} a été interrompu. Réessaie.`],
  ];
  for (const [what, message] of cases) {
    const b = bench({ types: { b1: "image" }, reread: async () => what });
    enterFiles(PAGE);
    const done = settled(b.upload(png(), "b1"));
    await until(() => b.sent.length === 1, "l'envoi");
    b.controller.abort();
    expect((await done).error).toBeDefined();
    expect(b.sent[0].signal.aborted).toBe(true);
    await until(() => b.refused.length === 1, `le message (${message})`);
    expect(b.refused).toEqual([message]);
    expect(filesOf(PAGE).uploads).toEqual([]);
    leaveFiles(PAGE);
  }
  // De même quand la réponse du serveur arrive après le départ de l'éditeur : rien n'est posé.
  const late = bench({ types: { b1: "image" }, reread: async () => page(PAGE, { level: "comment" }) });
  let answer!: (info: FileInfo) => void;
  const upload = uploader({
    pageId: PAGE,
    limits: async () => ({ maxFileBytes: MIB }),
    blockType: () => "image",
    onRefused: (message) => late.refused.push(message),
    reread: async () => page(PAGE, { level: "comment" }),
    signal: late.controller.signal,
    upload: () => new Promise<FileInfo>((done) => (answer = done)),
  });
  const done = settled(upload(png(), "b1"));
  await until(() => answer !== undefined, "l'envoi");
  late.controller.abort();
  answer({ id: FILE, url: PATH, name: "photo.png", size: 3, mime: "image/png" });
  expect((await done).error).toBeDefined();
  await until(() => late.refused.length === 1, "le message");
  expect(late.refused).toEqual([`${cited("photo.png")} n'a pas été envoyé : tu ne peux plus modifier cette page.`]);
});

test("an_upload_that_does_not_end_well_gives_its_block_back_and_one_that_does_keeps_it", async () => {
  // Le bloc qu'un dépôt a posé porte le nom local du fichier, lu par tous : un envoi qui n'aboutit pas
  // le rend (`discard`), pour qu'il soit retiré s'il est encore tel que l'envoi l'a laissé (m7).
  const failing: [what: string, run: (b: ReturnType<typeof bench>) => Promise<unknown>][] = [
    ["trop gros", (b) => settled(b.upload(png("photo.png", MIB + 1), "b1"))],
    [
      "refusé par le serveur",
      async (b) => {
        const done = settled(b.upload(png(), "b1"));
        await until(() => b.sent.length === 1, "l'envoi");
        b.sent[0].fail(new ApiError(403, "forbidden", "non"));
        return done;
      },
    ],
    [
      "coupé",
      async (b) => {
        const done = settled(b.upload(png(), "b1"));
        await until(() => b.sent.length === 1, "l'envoi");
        b.sent[0].fail(new ApiError(0, null, "coupé"));
        return done;
      },
    ],
    [
      "annulé",
      async (b) => {
        enterFiles(PAGE);
        const done = settled(b.upload(png(), "b1"));
        await until(() => filesOf(PAGE).uploads.length === 1, "l'envoi en cours");
        filesOf(PAGE).uploads[0].cancel();
        return done;
      },
    ],
    [
      "une réponse qui n'est pas un fichier de la page",
      async (b) => {
        const done = settled(b.upload(png(), "b1"));
        await until(() => b.sent.length === 1, "l'envoi");
        b.sent[0].answer({ url: "javascript:alert(1)" });
        return done;
      },
    ],
  ];
  for (const [what, run] of failing) {
    const b = bench({ types: { b1: "image" } });
    await run(b);
    expect(b.discarded, what).toEqual(["b1:photo.png"]);
    leaveFiles(PAGE);
  }
  // La limite illisible : le bloc est rendu aussi.
  const unread: string[] = [];
  const upload = uploader({
    pageId: PAGE,
    limits: async () => {
      throw new ApiError(0, null, "coupé");
    },
    blockType: () => "image",
    onRefused: () => {},
    reread: async () => page(PAGE),
    discard: (id, name) => unread.push(`${id}:${name}`),
    signal: new AbortController().signal,
  });
  await settled(upload(png(), "b1"));
  expect(unread).toEqual(["b1:photo.png"]);
  // Témoin : un envoi qui aboutit garde son bloc.
  const b = bench({ types: { b1: "image" } });
  const done = b.upload(png(), "b1");
  await until(() => b.sent.length === 1, "l'envoi");
  b.sent[0].answer({});
  await done;
  expect(b.discarded).toEqual([]);
  // L'éditeur parti (page quittée, session recréée) : il n'écrit plus, rien ne lui est rendu.
  const gone = bench({ types: { b1: "image" } });
  const stopped = settled(gone.upload(png(), "b1"));
  await until(() => gone.sent.length === 1, "l'envoi");
  gone.controller.abort();
  await stopped;
  await until(() => gone.refused.length === 1, "le message");
  expect(gone.discarded).toEqual([]);
});

test("sound_and_video_share_their_containers_and_the_server_type_picks_between_their_blocks", async () => {
  // Le serveur lit `video/webm` d'un son WebM et `audio/ogg` d'une vidéo Ogg (un seul conteneur) :
  // le bloc choisi par l'éditeur reste. Ailleurs, son type choisit entre le bloc son et le bloc vidéo,
  // sans tomber sur un bloc fichier (m9).
  const cases: [block: string, mime: string, expected: string | undefined][] = [
    ["audio", "video/webm", undefined],
    ["video", "audio/ogg", undefined],
    ["video", "video/webm", undefined],
    ["audio", "audio/ogg", undefined],
    ["video", "audio/mpeg", "audio"],
    ["video", "audio/mp4", "audio"],
    ["video", "audio/wav", "audio"],
    ["audio", "video/mp4", "video"],
    // Une image n'est ni l'un ni l'autre, et inversement.
    ["image", "video/webm", "file"],
    ["image", "audio/ogg", "file"],
    ["video", "image/png", "file"],
    ["audio", "image/png", "file"],
    ["video", "application/octet-stream", "file"],
    ["audio", "application/pdf", "file"],
  ];
  for (const [block, mime, expected] of cases) {
    const b = bench({ types: { b1: block } });
    const done = b.upload(png(), "b1");
    await until(() => b.sent.length === 1, "l'envoi");
    b.sent[0].answer({ mime });
    const props = { url: PATH, name: "photo.png" };
    expect(await done, `${block} ${mime}`).toEqual(expected === undefined ? { props } : { type: expected, props });
  }
});

test("a_note_of_a_page_is_shown_a_while_then_leaves_and_leaves_with_the_page", async () => {
  // Task C9 : un fichier d'une autre page rattaché à celle-ci se dit par une nouvelle, qui part seule.
  const { notesOf, sayNote } = await import("./files");
  sayNote(PAGE, "avant l'ouverture");
  expect(notesOf(PAGE)).toEqual([]);
  enterFiles(PAGE);
  let changes = 0;
  const stop = watchFiles(() => (changes += 1));
  sayNote(PAGE, "une", 30);
  sayNote(PAGE, "deux", 60_000);
  expect(notesOf(PAGE).map((note) => note.text)).toEqual(["une", "deux"]);
  expect(notesOf(PAGE)).toBe(notesOf(PAGE));
  // Ce n'est ni un refus, ni un envoi.
  expect(filesOf(PAGE)).toEqual({ uploads: [], refusals: [] });
  await until(() => notesOf(PAGE).length === 1, "la première nouvelle partie");
  expect(notesOf(PAGE).map((note) => note.text)).toEqual(["deux"]);
  expect(changes).toBe(3);
  // La page quittée : plus rien, et le minuteur de la seconde ne dit plus rien.
  leaveFiles(PAGE);
  expect(notesOf(PAGE)).toEqual([]);
  sayNote(PAGE, "après");
  expect(notesOf(PAGE)).toEqual([]);
  stop();
});
