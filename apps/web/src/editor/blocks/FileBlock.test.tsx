// Les blocs de fichier dans l'éditeur rendu (Task C8) : ce qu'ils chargent (`shownUrl`, jamais une
// adresse du document telle quelle), le bloc fichier et son lien « Télécharger », offert aussi à qui
// ne fait que lire, l'aperçu d'une version, et ce que l'éditeur fait d'un envoi.
import { act } from "react";
import { expect, test } from "vitest";
import * as Y from "yjs";
import { FRAGMENT } from "../collab";
import { EDITOR_OPTIONS, PreviewEditor } from "../Editor";
import { BLANK } from "../files";
import { REMOTE, seedState, type Seed } from "../../test/editors";
import { until } from "../../test/hooks";
import { blockOf, flush, foreignStyles, mountEditor, press, type Mounted } from "../../test/richEditor";
import { focusBlockLinks } from "./Toc";

const PAGE = "0192f0c1-0000-7000-8000-000000000001";
const PATH = `/api/v1/pages/${PAGE}/files/0192f0c1-0000-7000-8000-0000000000f1`;

const fileBlock = (props: Record<string, unknown>): Seed => [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "f", type: "file", props } as Seed[number],
  { id: "z", type: "paragraph", content: "Bloc Z" },
];

const download = (m: Mounted) => blockOf(m, "f").querySelector<HTMLAnchorElement>("a.mm-file-download");

test("file_block_offers_download_to_a_reader", async () => {
  for (const editable of [false, true]) {
    const m = await mountEditor({ blocks: fileBlock({ url: PATH, name: "rapport final.pdf" }), editable });
    const link = download(m);
    expect(link, `modifiable : ${editable}`).not.toBeNull();
    // Un vrai lien, vers l'adresse permise, que le navigateur télécharge sans quitter la page.
    expect(link!.getAttribute("href")).toBe(PATH);
    expect(link!.getAttribute("download")).toBe("rapport final.pdf");
    expect(link!.hasAttribute("target")).toBe(false);
    expect(link!.textContent).toBe("Télécharger");
    // Nommé avec le nom du fichier, isolé dans l'attribut.
    expect(link!.getAttribute("aria-label")).toBe("Télécharger \u2068rapport final.pdf\u2069");
    // Un îlot : ni ProseMirror ni l'interception des liens de l'application ne le prennent.
    expect(link!.getAttribute("contenteditable")).toBe("false");
    expect(m.errors).toEqual([]);
  }
});

test("a_file_at_a_web_address_opens_in_another_tab_without_its_opener", async () => {
  const m = await mountEditor({
    blocks: fileBlock({ url: "https://exemple.fr/a.pdf", name: "a.pdf" }),
    editable: false,
  });
  const link = download(m)!;
  expect(link.getAttribute("href")).toBe("https://exemple.fr/a.pdf");
  expect(link.getAttribute("target")).toBe("_blank");
  expect(link.getAttribute("rel")).toBe("noopener noreferrer");
  // `download` ne vaut pas d'une origine à l'autre : le lien dit ce qu'il fait.
  expect(link.hasAttribute("download")).toBe(false);
  expect(link.textContent).toBe("Ouvrir");
  // Le lien porte l'adresse telle que `shownUrl` la rend, pas celle du document.
  const loose = await mountEditor({
    blocks: fileBlock({ url: "  https://EXEMPLE.fr/a b.pdf ", name: "a b.pdf" }),
    editable: false,
  });
  expect(download(loose)!.getAttribute("href")).toBe("https://exemple.fr/a%20b.pdf");
});

test("file_block_offers_nothing_when_its_address_is_not_permitted", async () => {
  const addresses: unknown[] = [
    "javascript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "blob:http://localhost/1",
    "/api/v1/auth/logout",
    `${PATH}?x`,
    `//ailleurs.example${PATH}`,
    "fichier.pdf",
  ];
  for (const url of addresses) {
    const m = await mountEditor({ blocks: fileBlock({ url, name: "piège.pdf" }), editable: false });
    const block = blockOf(m, "f");
    expect(block.querySelector("a"), String(url)).toBeNull();
    expect(block.querySelector("[href], [src]"), String(url)).toBeNull();
    // Le nom reste montré, et le bloc dit pourquoi il n'offre rien.
    expect(block.textContent, String(url)).toContain("piège.pdf");
    expect(block.querySelector(".mm-file-unavailable")?.textContent, String(url)).toBe("Adresse non prise en charge");
    expect(m.errors).toEqual([]);
  }
});

test("file_name_is_isolated_text", async () => {
  const name = '<img src=x onerror="window.__ran = 1">\u202Egnp.exe';
  const m = await mountEditor({ blocks: fileBlock({ url: PATH, name }), editable: false });
  const block = blockOf(m, "f");
  const shown = block.querySelector<HTMLElement>(".bn-file-name")!;
  // Du texte, jamais du HTML ; isolé (`dir="auto"`, et `unicode-bidi: isolate` par la feuille de style).
  expect(shown.textContent).toBe(name);
  expect(shown.children).toHaveLength(0);
  expect(shown.getAttribute("dir")).toBe("auto");
  expect(block.querySelector("img")).toBeNull();
  expect((window as { __ran?: number }).__ran).toBeUndefined();
  expect(foreignStyles(block)).toEqual([]);
  // Dans l'attribut, le nom ne ferme pas son isolement et ne retourne pas le reste.
  const label = download(m)!.getAttribute("aria-label")!;
  expect(label.startsWith("Télécharger \u2068<img")).toBe(true);
  expect(label).not.toContain("\u202E");
  expect([...label].filter((c) => c === "\u2069")).toHaveLength(1);
});

test("a_file_block_survives_props_that_are_not_text", async () => {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, await seedState(fileBlock({ url: PATH, name: "a.pdf" })), REMOTE);
  const m = await mountEditor({ doc, editable: false });
  // Un autre client écrit des objets à la place des chaînes (Yjs garde toute valeur JSON).
  const content = (
    (doc.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement).toArray()[1] as Y.XmlElement
  ).get(0) as Y.XmlElement;
  expect(content.nodeName).toBe("file");
  await act(async () => {
    doc.transact(() => {
      content.setAttribute("url", { href: PATH } as unknown as string);
      content.setAttribute("name", { nom: "x" } as unknown as string);
      content.setAttribute("caption", ["<b>"] as unknown as string);
    }, REMOTE);
  });
  await flush();
  const block = blockOf(m, "f");
  expect(block.querySelector("a")).toBeNull();
  expect(block.textContent).not.toContain("[object Object]");
  expect(m.errors).toEqual([]);
  expect(m.text()).toContain("Bloc Z");
});

test("an_empty_file_block_offers_to_add_a_file_only_to_who_can_write", async () => {
  const reader = await mountEditor({ blocks: fileBlock({}), editable: false });
  expect(download(reader)).toBeNull();
  expect(blockOf(reader, "f").querySelector(".mm-file-unavailable")).toBeNull();
  const writer = await mountEditor({ blocks: fileBlock({}), editable: true });
  expect(blockOf(writer, "f").querySelector(".bn-add-file-button")).not.toBeNull();
});

test("the_file_panel_still_opens_on_a_file_block", async () => {
  // Le bloc `file` récrit garde ce que BlockNote lit de lui : le panneau de fichier s'ouvre sur un
  // bloc sans adresse, et propose tout type de fichier.
  const spec = EDITOR_OPTIONS.schema.blockSpecs.file;
  expect(spec.implementation.meta?.fileBlockAccept).toEqual(["*/*"]);
  expect(typeof spec.implementation.parse).toBe("function");
  expect(typeof spec.implementation.toExternalHTML).toBe("function");
  const m = await mountEditor({ blocks: fileBlock({}), editable: true, uploadFile: async () => PATH });
  const add = blockOf(m, "f").querySelector<HTMLElement>(".bn-add-file-button")!;
  await act(async () => {
    add.click();
  });
  await until(m.host, () => document.querySelector(".bn-panel") !== null, "le panneau de fichier");
  // L'onglet d'envoi est là quand l'éditeur sait envoyer.
  expect(document.querySelector('.bn-panel input[type="file"], .bn-panel [data-test="upload-input"]')).not.toBeNull();
});

test("a_copied_file_block_carries_only_a_permitted_address", async () => {
  const toHtml = EDITOR_OPTIONS.schema.blockSpecs.file.implementation.toExternalHTML!;
  const html = (url: string) => {
    const host = document.createElement("div");
    const out = (toHtml as (block: unknown, editor: unknown) => { dom: Node } | undefined).call(
      {},
      { id: "f", type: "file", props: { url, name: "a.pdf", caption: "", backgroundColor: "default" }, children: [] },
      {},
    );
    host.append(out!.dom);
    return host;
  };
  expect(html(PATH).querySelector("a")?.getAttribute("href")).toBe(PATH);
  expect(html("https://exemple.fr/a.pdf").querySelector("a")?.getAttribute("href")).toBe("https://exemple.fr/a.pdf");
  for (const url of ["javascript:alert(1)", "data:text/html,x", "/api/v1/auth/logout"]) {
    const out = html(url);
    expect(out.querySelector("[href], [src]"), url).toBeNull();
    expect(out.textContent, url).toBe("a.pdf");
  }
});

// Les blocs de BlockNote (image, vidéo, son) chargent ce que rend `resolveFileUrl` : `shownUrl`.

const media = (type: "image" | "video" | "audio", url: unknown): Seed => [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "f", type, props: { url, name: "média" } } as Seed[number],
];

test("an_image_a_video_or_a_sound_is_loaded_only_from_a_permitted_address", async () => {
  const tags = { image: "img", video: "video", audio: "audio" } as const;
  for (const type of ["image", "video", "audio"] as const) {
    const cases: [url: string, src: string][] = [
      [PATH, PATH],
      ["https://exemple.fr/a.png", "https://exemple.fr/a.png"],
      ["javascript:alert(1)", BLANK],
      ["data:image/svg+xml,<svg onload=alert(1)>", BLANK],
      ["/api/v1/auth/logout", BLANK],
      [`${PATH}?x`, BLANK],
      [`//ailleurs.example${PATH}`, BLANK],
    ];
    for (const [url, src] of cases) {
      const m = await mountEditor({ blocks: media(type, url), editable: false });
      const element = () => blockOf(m, "f").querySelector(tags[type]);
      await until(m.host, () => element()?.getAttribute("src") != null, `${type} : le src de ${url}`);
      expect(element()!.getAttribute("src"), `${type} ${url}`).toBe(src);
      // Rien d'autre dans le bloc ne porte l'adresse du document.
      const carried = [...blockOf(m, "f").querySelectorAll("[href], [src], [data], [poster]")].map(
        (e) => e.getAttribute("href") ?? e.getAttribute("src") ?? e.getAttribute("data") ?? e.getAttribute("poster"),
      );
      expect(carried, `${type} ${url}`).toEqual([src]);
    }
  }
});

test("a_version_preview_resolves_its_urls_and_never_uploads", async () => {
  // Les options communes à l'éditeur de la page et à l'aperçu : de quoi lire, rien pour envoyer.
  expect("uploadFile" in EDITOR_OPTIONS).toBe(false);
  expect(await EDITOR_OPTIONS.resolveFileUrl("javascript:alert(1)")).toBe(BLANK);
  expect(await EDITOR_OPTIONS.resolveFileUrl(PATH)).toBe(PATH);
  const doc = new Y.Doc();
  Y.applyUpdate(
    doc,
    await seedState([
      { id: "ok", type: "image", props: { url: PATH, name: "a.png" } },
      { id: "bad", type: "image", props: { url: "javascript:alert(1)", name: "b.png" } },
      { id: "empty", type: "image", props: {} },
      { id: "f", type: "file", props: { url: PATH, name: "a.pdf" } },
    ] as Seed),
    REMOTE,
  );
  const m = await mountEditor({ view: () => <PreviewEditor doc={doc} /> });
  const src = (id: string) => blockOf(m, id).querySelector("img")?.getAttribute("src");
  await until(m.host, () => src("ok") != null && src("bad") != null, "les images de l'aperçu");
  expect(src("ok")).toBe(PATH);
  expect(src("bad")).toBe(BLANK);
  // Un fichier d'une version se télécharge aussi.
  expect(download(m)?.getAttribute("href")).toBe(PATH);
  // Ni panneau de fichier, ni dépôt : un clic sur un bloc sans fichier n'ouvre rien.
  const add = blockOf(m, "empty").querySelector<HTMLElement>(".bn-add-file-button");
  await act(async () => {
    add?.click();
  });
  await flush();
  expect(document.querySelector(".bn-panel")).toBeNull();
  expect(document.querySelector('input[type="file"]')).toBeNull();
});

test("ctrl_alt_enter_on_a_file_block_goes_to_its_download_link", async () => {
  const m = await mountEditor({ blocks: fileBlock({ url: PATH, name: "a.pdf" }), editable: true });
  await act(async () => {
    m.editor.focus();
    m.editor.setTextCursorPosition("f");
  });
  expect(focusBlockLinks(m.editor)).toBe(true);
  expect(document.activeElement).toBe(download(m));
  // Entrée sur le lien n'est pas une frappe du document.
  const before = m.writes.length;
  press(download(m)!, "Enter");
  await flush();
  expect(m.writes.length).toBe(before);
  expect(m.editor.document.map((b) => b.id)).toEqual(["x", "f", "z"]);
});
