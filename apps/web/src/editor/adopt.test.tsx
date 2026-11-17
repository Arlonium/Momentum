// Un fichier venu d'une autre page (Task C9, R249) : un bloc collé garde l'adresse de sa première
// page ; l'éditeur fait recopier l'attache dans la page où l'on colle, puis récrit l'adresse du bloc.
// Le serveur est simulé (`copy`), le collage est celui de BlockNote (`pasteHTML`), donné comme le
// presse-papiers le donne.
import type { FileInfo, PageNode } from "@mementum/protocol";
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import type { Seed } from "../test/editors";
import { page } from "../test/pages";
import { mountEditor, type Mounted } from "../test/richEditor";
import { adoptingPaste, foreignFiles, MAX_ADOPTED_FILES, type AdoptOptions } from "./adopt";
import * as scan from "./fileBlocks";

vi.mock("./fileBlocks", async (original) => {
  const real = await original<typeof import("./fileBlocks")>();
  return { ...real, blocksNaming: vi.fn(real.blocksNaming) };
});

afterEach(() => vi.mocked(scan.blocksNaming).mockClear());

// jsdom n'a pas `ClipboardEvent`, que ProseMirror construit pour coller du HTML.
(globalThis as { ClipboardEvent?: unknown }).ClipboardEvent ??= class extends Event {};

const HERE = "0192f0c1-0000-7000-8000-000000000001";
const THERE = "0192f0c1-0000-7000-8000-000000000002";
const uuid = (n: number) => `0192f0c1-0000-7000-8000-${String(n).padStart(12, "0")}`;
const pathOf = (pageId: string, n: number) => `/api/v1/pages/${pageId}/files/${uuid(n)}`;
const FOREIGN = pathOf(THERE, 101);
const OWN = pathOf(HERE, 201);

type Call = { pageId: string; source: { pageId: string; fileId: string }; answer(info?: Partial<FileInfo>): void; fail(error: unknown): void };

/** Un éditeur de la page `HERE`, et le serveur qui recopie quand le test le veut. */
async function bench(blocks: Seed, { editable = true, reread = async (): Promise<PageNode | null | undefined> => page(HERE) } = {}) {
  const m = await mountEditor({ blocks, editable });
  const calls: Call[] = [];
  const said: string[] = [];
  const state = { rereads: 0, made: 0 };
  const controller = new AbortController();
  const options: AdoptOptions = {
    pageId: HERE,
    signal: controller.signal,
    onRefused: (message) => said.push(message),
    reread: () => {
      state.rereads += 1;
      return reread();
    },
    copy: (pageId, source) =>
      new Promise<FileInfo>((done, fail) => {
        calls.push({
          pageId,
          source,
          answer: (info = {}) => {
            state.made += 1;
            done({ id: uuid(900 + state.made), url: pathOf(HERE, 900 + state.made), name: "a.png", size: 3, mime: "image/png", ...info });
          },
          fail,
        });
      }),
  };
  const handler = adoptingPaste(options);
  /** Colle `data` (par type) au curseur, comme le presse-papiers : le collage de BlockNote, puis le nôtre. */
  const paste = async (data: Record<string, string>) => {
    const event = {
      clipboardData: { types: Object.keys(data), getData: (type: string) => data[type] ?? "" },
    } as unknown as ClipboardEvent;
    await act(async () => {
      m.editor.focus();
      m.editor.setTextCursorPosition("x", "end");
      handler({
        event,
        editor: m.editor,
        defaultPasteHandler: () => {
          if (data["blocknote/html"] !== undefined) m.editor.pasteHTML(data["blocknote/html"], true);
          else if (data["text/html"] !== undefined) m.editor.pasteHTML(data["text/html"]);
          else m.editor.pasteText(data["text/plain"] ?? "");
          return true;
        },
      });
    });
  };
  return { m, calls, said, state, controller, paste, options };
}

const X: Seed = [{ id: "x", type: "paragraph", content: "Bloc X" }];
/** Ce que la copie de `blocks` met dans le presse-papiers (le HTML interne de BlockNote). */
async function copied(blocks: unknown[]): Promise<Record<string, string>> {
  const source = await mountEditor({ blocks: blocks as Seed, editable: true });
  return { "blocknote/html": source.editor.blocksToFullHTML(source.editor.document) };
}
const image = (url: string, extra: Record<string, unknown> = {}) => ({ type: "image", props: { url, name: "a.png", ...extra } });

/** Les blocs de fichier de `m` : leur sorte et leur adresse, dans l'ordre. */
function fileUrls(m: Mounted): [string, unknown][] {
  const out: [string, unknown][] = [];
  const walk = (blocks: Mounted["editor"]["document"]) => {
    for (const block of blocks) {
      if (["image", "video", "audio", "file"].includes(block.type)) out.push([block.type, (block.props as { url?: unknown }).url]);
      walk(block.children);
    }
  };
  walk(m.editor.document);
  return out;
}

/** Tous les blocs de `m`, à toute profondeur, dans l'ordre du document. */
function all(m: Mounted): Mounted["editor"]["document"] {
  const walk = (blocks: Mounted["editor"]["document"]): Mounted["editor"]["document"] =>
    blocks.flatMap((block) => [block, ...walk(block.children)]);
  return walk(m.editor.document);
}

async function until(condition: () => boolean, what: string): Promise<void> {
  const end = performance.now() + 2_000;
  while (!condition()) {
    if (performance.now() > end) throw new Error(`jamais vu : ${what}`);
    await act(async () => {
      await new Promise((done) => setTimeout(done, 0));
    });
  }
}

test("foreignFiles_finds_only_the_exact_paths_of_files_of_another_page", () => {
  const text = [
    `<img src="${FOREIGN}">`,
    `data-url="${pathOf(THERE, 102)}"`,
    FOREIGN,
    OWN,
    `https://ailleurs.example${pathOf(THERE, 103)}`,
    `${pathOf(THERE, 104)}?x`,
    `${pathOf(THERE, 105)}/../x`,
    `/api/v1/pages/${THERE}/files/copies`,
    "javascript:alert(1)",
    `x${pathOf(THERE, 106)}`,
    `/API/v1/pages/${THERE}/files/${uuid(107)}`,
    `&quot;${pathOf(THERE.toUpperCase(), 108)}&quot;`,
  ].join("\n");
  const found = foreignFiles([text, `![](${pathOf(THERE, 109)})`], HERE);
  // Seulement des chemins entiers, bornés (un guillemet, une parenthèse, une espace, un bord) ; ni
  // ceux de cette page, ni une adresse à hôte, à requête ou à suite.
  expect([...found.keys()].sort()).toEqual(
    [FOREIGN, pathOf(THERE, 102), pathOf(THERE.toUpperCase(), 108), pathOf(THERE, 109)].sort(),
  );
  expect(found.get(FOREIGN)).toEqual({ pageId: THERE, fileId: uuid(101) });
  // La page d'ici, écrite dans une autre casse, reste la page d'ici.
  expect(foreignFiles([pathOf(HERE.toUpperCase(), 1)], HERE).size).toBe(0);
});

test("a_pasted_file_of_another_page_is_copied_here_and_its_url_rewritten", async () => {
  for (const type of ["image", "video", "audio", "file"]) {
    const b = await bench(X);
    await b.paste(await copied([{ type, props: { url: FOREIGN, name: "a.png", caption: "Vue du port" } }]));
    // Le bloc collé porte d'abord l'adresse de sa première page.
    expect(fileUrls(b.m), type).toEqual([[type, FOREIGN]]);
    await until(() => b.calls.length === 1, "la recopie");
    expect(b.calls[0].pageId).toBe(HERE);
    expect(b.calls[0].source).toEqual({ pageId: THERE, fileId: uuid(101) });
    const before = b.m.writes.length;
    await act(async () => b.calls[0].answer());
    await until(() => fileUrls(b.m)[0][1] !== FOREIGN, "l'adresse récrite");
    expect(fileUrls(b.m), type).toEqual([[type, pathOf(HERE, 901)]]);
    // Rien d'autre du bloc n'a changé, et rien n'est dit.
    const block = all(b.m).find((candidate) => candidate.type === type)!;
    expect(block.props).toMatchObject({ name: "a.png", caption: "Vue du port" });
    expect(b.m.writes.length).toBe(before + 1);
    expect(b.said).toEqual([]);
    expect(b.m.errors).toEqual([]);
  }
});

test("pasted_blocks_naming_the_same_file_share_one_copy_and_each_file_has_its_own", async () => {
  const b = await bench(X);
  await b.paste(
    await copied([
      image(FOREIGN),
      { type: "paragraph", content: "Entre deux", children: [image(FOREIGN)] },
      image(pathOf(THERE, 102)),
    ]),
  );
  // Une recopie à la fois, une par fichier.
  await until(() => b.calls.length === 1, "la première recopie");
  await new Promise((done) => setTimeout(done, 20));
  expect(b.calls).toHaveLength(1);
  await act(async () => b.calls[0].answer());
  await until(() => b.calls.length === 2, "la seconde recopie");
  await act(async () => b.calls[1].answer());
  await until(() => fileUrls(b.m).every(([, url]) => url !== FOREIGN && url !== pathOf(THERE, 102)), "les adresses récrites");
  expect(b.calls.map((call) => call.source.fileId)).toEqual([uuid(101), uuid(102)]);
  expect(fileUrls(b.m).map(([, url]) => url)).toEqual([pathOf(HERE, 901), pathOf(HERE, 901), pathOf(HERE, 902)]);
});

test("the_document_is_scanned_only_after_a_paste_that_carries_a_file_path", async () => {
  // Un bloc d'une autre page était déjà là (écrit par MMD, un agent) : il n'est pas de ce collage.
  const b = await bench([...X, { id: "old", ...image(FOREIGN) }] as Seed);
  // Une frappe, un collage de texte, d'un fichier de cette page, d'une adresse web : aucun parcours.
  await act(async () => {
    b.m.editor.focus();
    b.m.editor.setTextCursorPosition("x", "end");
    b.m.editor.insertInlineContent(" tapé");
  });
  await b.paste({ "text/plain": "du texte, et /api/v1/pages/x/files/y" });
  await b.paste(await copied([image(OWN), image("https://exemple.fr/a.png")]));
  expect(vi.mocked(scan.blocksNaming)).not.toHaveBeenCalled();
  expect(b.calls).toEqual([]);
  // Témoin : un collage qui porte le chemin d'un fichier d'une autre page parcourt la page, avant et après.
  await b.paste(await copied([image(FOREIGN)]));
  expect(vi.mocked(scan.blocksNaming)).toHaveBeenCalledTimes(2);
  await until(() => b.calls.length === 1, "la recopie");
  await act(async () => b.calls[0].answer());
  await until(() => fileUrls(b.m).some(([, url]) => url === pathOf(HERE, 901)), "l'adresse récrite");
  // Le bloc qui était là avant le collage garde son adresse : personne n'a demandé de le changer.
  expect(b.m.editor.getBlock("old")!.props).toMatchObject({ url: FOREIGN });
  expect(fileUrls(b.m).filter(([, url]) => url === pathOf(HERE, 901))).toHaveLength(1);
});

test("a_refused_copy_leaves_the_block_as_it_is", async () => {
  // `422 invalid_source` : la page d'origine ne se lit pas, ou le fichier n'en est plus. Le bloc garde
  // son adresse, que la route juge à chaque lecture ; rien n'est dit de plus que ce que le serveur dit.
  const b = await bench(X);
  await b.paste(await copied([image(FOREIGN), image(pathOf(THERE, 102))]));
  await until(() => b.calls.length === 1, "la recopie");
  const before = b.m.writes.length;
  await act(async () => b.calls[0].fail(new ApiError(422, "invalid_source", "fichier d'origine introuvable")));
  // La suivante part quand même, et aboutit.
  await until(() => b.calls.length === 2, "la recopie suivante");
  expect(b.m.writes.length).toBe(before);
  await act(async () => b.calls[1].answer());
  await until(() => fileUrls(b.m)[1][1] !== pathOf(THERE, 102), "la seconde adresse récrite");
  expect(fileUrls(b.m).map(([, url]) => url)).toEqual([FOREIGN, pathOf(HERE, 901)]);
  expect(b.said).toEqual([]);
  expect(b.state.rereads).toBe(0);
});

test("a_copy_refused_to_the_account_stops_the_others_is_said_and_the_page_is_read_again", async () => {
  const refusals: [ApiError, string][] = [
    [new ApiError(403, "forbidden", "il te faut « Peut modifier » sur cette page"), " Le serveur dit : il te faut « Peut modifier » sur cette page"],
    [new ApiError(409, "trashed", "la page est à la corbeille"), " Le serveur dit : la page est à la corbeille"],
    [new ApiError(0, null, "Le serveur Mementum ne répond pas."), ""],
    [new ApiError(502, null, "Le serveur Mementum ne répond pas."), ""],
  ];
  for (const [refusal, tail] of refusals) {
    const b = await bench(X);
    await b.paste(await copied([image(FOREIGN), image(pathOf(THERE, 102)), image(pathOf(THERE, 103))]));
    await until(() => b.calls.length === 1, "la recopie");
    await act(async () => b.calls[0].fail(refusal));
    await until(() => b.said.length === 1, "le message");
    expect(b.said, String(refusal.status)).toEqual([
      `3 fichiers collés n'ont pas pu être rattachés à cette page : ils restent ceux de leur page d'origine, et ne seront vus que de qui peut la lire.${tail}`,
    ]);
    // Les autres ne partent pas ; les blocs gardent leur adresse.
    await new Promise((done) => setTimeout(done, 20));
    expect(b.calls).toHaveLength(1);
    expect(fileUrls(b.m).map(([, url]) => url)).toEqual([FOREIGN, pathOf(THERE, 102), pathOf(THERE, 103)]);
    // Un refus fait au compte : un droit, ou la page, ont changé.
    expect(b.state.rereads, String(refusal.status)).toBe(refusal.status === 403 || refusal.status === 409 ? 1 : 0);
  }
  // Un seul fichier : au singulier.
  const one = await bench(X);
  await one.paste(await copied([image(FOREIGN)]));
  await until(() => one.calls.length === 1, "la recopie");
  await act(async () => one.calls[0].fail(new ApiError(0, null, "coupé")));
  await until(() => one.said.length === 1, "le message");
  expect(one.said).toEqual([
    "Un fichier collé n'a pas pu être rattaché à cette page : il reste celui de sa page d'origine, et ne sera vu que de qui peut la lire.",
  ]);
});

test("a_copy_refused_for_a_lost_session_is_made_again_once_the_account_is_back", async () => {
  const b = await bench(X);
  await b.paste(await copied([image(FOREIGN)]));
  await until(() => b.calls.length === 1, "la recopie");
  await act(async () => b.calls[0].fail(new ApiError(401, "unauthenticated", "session absente")));
  // La page est relue (elle attend le retour du compte), puis la recopie repart, une fois.
  await until(() => b.calls.length === 2, "la recopie refaite");
  expect(b.state.rereads).toBe(1);
  await act(async () => b.calls[1].fail(new ApiError(401, "unauthenticated", "session absente")));
  await until(() => b.said.length === 1, "le message");
  await new Promise((done) => setTimeout(done, 20));
  expect(b.calls).toHaveLength(2);
  expect(fileUrls(b.m)).toEqual([["image", FOREIGN]]);
  // Témoin : au retour du compte, elle aboutit.
  const back = await bench(X);
  await back.paste(await copied([image(FOREIGN)]));
  await until(() => back.calls.length === 1, "la recopie");
  await act(async () => back.calls[0].fail(new ApiError(401, "unauthenticated", "session absente")));
  await until(() => back.calls.length === 2, "la recopie refaite");
  await act(async () => back.calls[1].answer());
  await until(() => fileUrls(back.m)[0][1] === pathOf(HERE, 901), "l'adresse récrite");
  expect(back.said).toEqual([]);
  // La page n'est plus modifiable au retour : rien ne repart.
  const lost = await bench(X, { reread: async () => page(HERE, { level: "read" }) });
  await lost.paste(await copied([image(FOREIGN)]));
  await until(() => lost.calls.length === 1, "la recopie");
  await act(async () => lost.calls[0].fail(new ApiError(401, "unauthenticated", "session absente")));
  await until(() => lost.said.length === 1, "le message");
  expect(lost.calls).toHaveLength(1);
});

test("a_file_of_this_page_and_an_external_address_are_left_alone", async () => {
  const b = await bench(X);
  const others = [
    OWN,
    "https://exemple.fr/a.png",
    `https://ailleurs.example${FOREIGN}`,
    `${FOREIGN}?x`,
    "javascript:alert(1)",
    "",
  ];
  // Avec eux, un fichier d'une autre page : lui seul est recopié.
  await b.paste(await copied([...others.map((url) => image(url)), image(pathOf(THERE, 102))]));
  await until(() => b.calls.length === 1, "la recopie");
  expect(b.calls[0].source.fileId).toBe(uuid(102));
  await act(async () => b.calls[0].answer());
  await until(() => fileUrls(b.m).some(([, url]) => url === pathOf(HERE, 901)), "l'adresse récrite");
  await new Promise((done) => setTimeout(done, 20));
  expect(b.calls).toHaveLength(1);
  expect(fileUrls(b.m).map(([, url]) => url)).toEqual([...others, pathOf(HERE, 901)]);
});

test("what_the_server_answers_is_set_only_if_it_is_a_file_of_this_page", async () => {
  for (const url of [pathOf(THERE, 500), "javascript:alert(1)", `https://ailleurs.example${OWN}`, 7 as unknown as string]) {
    const b = await bench(X);
    await b.paste(await copied([image(FOREIGN)]));
    await until(() => b.calls.length === 1, "la recopie");
    const before = b.m.writes.length;
    await act(async () => b.calls[0].answer({ url }));
    await until(() => b.said.length === 1, "le message");
    expect(fileUrls(b.m), String(url)).toEqual([["image", FOREIGN]]);
    expect(b.m.writes.length).toBe(before);
  }
});

test("a_block_changed_or_gone_during_the_copy_is_left_alone", async () => {
  // Pendant la recopie, la personne (ou un autre) a changé l'adresse du bloc, ou l'a supprimé.
  const b = await bench(X);
  await b.paste(await copied([image(FOREIGN), image(FOREIGN), image(FOREIGN)]));
  await until(() => b.calls.length === 1, "la recopie");
  const [changed, gone, kept] = all(b.m).filter((block) => block.type === "image");
  await act(async () => {
    b.m.editor.updateBlock(changed.id, { props: { url: "https://exemple.fr/autre.png" } } as never);
    b.m.editor.removeBlocks([gone.id]);
  });
  await act(async () => b.calls[0].answer());
  await until(() => (b.m.editor.getBlock(kept.id)!.props as { url: string }).url === pathOf(HERE, 901), "l'adresse récrite");
  expect(fileUrls(b.m).map(([, url]) => url)).toEqual(["https://exemple.fr/autre.png", pathOf(HERE, 901)]);
  expect(b.m.errors).toEqual([]);
});

test("nothing_is_copied_from_a_read_only_editor", async () => {
  // Un éditeur qui n'écrit pas (lecteur, corbeille, autre schéma) : ni parcours, ni recopie, même si
  // un collage lui arrivait ; et un éditeur parti pendant la recopie n'écrit plus rien.
  const reader = await bench([...X, { id: "old", ...image(FOREIGN) }] as Seed, { editable: false });
  await reader.paste(await copied([image(FOREIGN)]));
  await new Promise((done) => setTimeout(done, 20));
  expect(reader.calls).toEqual([]);
  expect(vi.mocked(scan.blocksNaming)).not.toHaveBeenCalled();
  const gone = await bench(X);
  await gone.paste(await copied([image(FOREIGN), image(pathOf(THERE, 102))]));
  await until(() => gone.calls.length === 1, "la recopie");
  const before = gone.m.writes.length;
  gone.controller.abort();
  await act(async () => gone.calls[0].answer());
  await new Promise((done) => setTimeout(done, 20));
  expect(gone.m.writes.length).toBe(before);
  expect(gone.calls).toHaveLength(1);
  expect(gone.said).toEqual([]);
});

test("a_paste_of_more_files_than_the_bound_copies_the_first_and_says_so", async () => {
  const b = await bench(X);
  const many = Array.from({ length: MAX_ADOPTED_FILES + 3 }, (_, i) => image(pathOf(THERE, 1000 + i)));
  await b.paste(await copied(many));
  for (let i = 0; i < MAX_ADOPTED_FILES; i++) {
    await until(() => b.calls.length === i + 1, `la recopie ${i + 1}`);
    await act(async () => b.calls[i].answer());
  }
  await until(() => b.said.length === 1, "le message");
  await new Promise((done) => setTimeout(done, 20));
  expect(b.calls).toHaveLength(MAX_ADOPTED_FILES);
  expect(b.said).toEqual([
    `Plus de ${MAX_ADOPTED_FILES} fichiers d'autres pages collés d'un coup : les ${MAX_ADOPTED_FILES} premiers sont rattachés à cette page, les 3 autres restent ceux de leur page d'origine, et ne seront vus que de qui peut la lire.`,
  ]);
  const urls = fileUrls(b.m).map(([, url]) => String(url));
  expect(urls.filter((url) => url.includes(HERE))).toHaveLength(MAX_ADOPTED_FILES);
  expect(urls.slice(-3)).toEqual([1100, 1101, 1102].map((n) => pathOf(THERE, n)));
}, 30_000);
