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
  return { upload, sent, refused, state, controller, types };
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
  const done = b.upload(png("  photo‮.png"), "b1");
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
    ["image", "application/pdf", "file"],
    ["image", "imagery/png", "file"],
    ["video", "video/webm", "same"],
    ["video", "audio/ogg", "file"],
    ["audio", "audio/mpeg", "same"],
    ["audio", "video/mp4", "file"],
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
  const big = await settled(b.upload(png("gros ⁨fichier.png", 2 * MIB + 1), "b1"));
  expect(big.error).toBeDefined();
  expect(b.sent).toEqual([]);
  expect(b.refused).toEqual([
    "⁨gros fichier.png⁩ est trop gros : 2 Mio au plus par fichier sur cette instance. Rien n'a été envoyé.",
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
  expect(refused).toEqual(["⁨photo.png⁩ n'a pas été envoyé : Le serveur Mementum ne répond pas."]);
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
    expect(b.refused, String(status)).toEqual([`⁨photo.png⁩ n'a pas été envoyé : ${message}`]);
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
    [undefined, "⁨photo.png⁩ n'a pas été envoyé : le serveur ne répond pas. Vérifie ta connexion, puis réessaie."],
    [null, "⁨photo.png⁩ n'a pas été envoyé : tu n'as plus accès à cette page."],
    [page(PAGE, { level: "comment" }), "⁨photo.png⁩ n'a pas été envoyé : tu ne peux plus modifier cette page."],
    [page(PAGE, { level: "read" }), "⁨photo.png⁩ n'a pas été envoyé : tu ne peux plus modifier cette page."],
    [page(PAGE, { trashedAt: 1 }), "⁨photo.png⁩ n'a pas été envoyé : la page est à la corbeille."],
    [page(PAGE), "L'envoi de ⁨photo.png⁩ a été interrompu. Réessaie."],
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
  expect((await settled(b.upload(png(), "b1"))).error).toBeDefined();
  expect(b.sent).toHaveLength(1);
  // Une réponse arrivée quand même ne pose rien.
  b.sent[0].answer({});
  expect(filesOf(PAGE)).toEqual({ uploads: [], refusals: [] });
});

test("an_upload_stopped_because_the_session_of_the_page_is_made_again_is_said", async () => {
  const b = bench({ types: { b1: "image" } });
  enterFiles(PAGE);
  const said: string[] = [];
  const done = settled(
    uploader({
      pageId: PAGE,
      limits: async () => ({ maxFileBytes: MIB }),
      blockType: () => "image",
      onRefused: (message) => said.push(message),
      reread: async () => page(PAGE),
      signal: b.controller.signal,
      upload: (_page, _file, signal) =>
        new Promise<FileInfo>((_answer, fail) => {
          signal?.addEventListener("abort", () => fail(new DOMException("annulé", "AbortError")));
        }),
    })(png(), "b1"),
  );
  await until(() => filesOf(PAGE).uploads.length === 1, "l'envoi en cours");
  // L'éditeur est remplacé (droits baissés, corbeille), la page reste ouverte.
  b.controller.abort();
  expect((await done).error).toBeDefined();
  expect(said).toEqual(["L'envoi de ⁨photo.png⁩ a été interrompu. Réessaie."]);
  expect(filesOf(PAGE).uploads).toEqual([]);
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
    "⁨photo.png⁩ a été envoyé, mais son bloc a été supprimé entre-temps : il n'est pas dans la page.",
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
    expect(b.refused).toEqual(["⁨photo.png⁩ n'a pas été envoyé : le serveur a répondu autre chose qu'un fichier de cette page."]);
  }
});

test("a_long_or_misleading_file_name_is_cut_and_isolated_in_a_message", async () => {
  const b = bench({ max: 1, types: { b1: "image" } });
  await settled(b.upload(png(`${"a".repeat(300)}‮gnp.exe⁩⁩`, 2), "b1"));
  const [message] = b.refused;
  // Isolé : ni marque de direction du nom, ni isolement fermé par lui.
  expect(message.startsWith(`⁨${"a".repeat(80)}…⁩ est trop gros`)).toBe(true);
  expect([...message].filter((c) => c === "⁩")).toHaveLength(1);
  expect(message).not.toContain("‮");
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
