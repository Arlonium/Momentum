// Garde des attributs, C2 de la relecture (Ruling R705) : un tableau à cellule fusionnée, fait
// dans l'éditeur, copié puis collé dans la même page (le presse-papiers de BlockNote). La cellule
// collée porte `colwidth = [NaN, NaN]` : une largeur absente, que le greffon et le serveur
// gardent. Sonde du relecteur, gardée.
import { expect, test } from "@playwright/test";
import type {} from "../src/editor/debug";
import { closeContexts, createPage, defaultAccount, apiAs, getMmd, openPage, unique } from "./helpers";

test.afterEach(closeContexts);

test("a_table_with_a_merged_cell_is_copied_and_pasted", async ({ browser }) => {
  const api = await apiAs(defaultAccount());
  const page = await createPage(api, { title: `Copie ${unique()}` });
  const p = await openPage(browser, page.id);
  const errors: string[] = [];
  p.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
  await p.locator(".bn-editor").click();
  await p.keyboard.type("avant");
  // Le tableau, par l'API de l'éditeur (ce que fait « Fusionner les cellules »).
  const inserted = await p.evaluate(() => {
    const editor = window.__mementum!.editor as unknown as {
      document: { id: string; type: string }[];
      insertBlocks(blocks: unknown[], ref: string, place: string): void;
    };
    editor.insertBlocks(
      [
        {
          id: "tableau",
          type: "table",
          content: {
            type: "tableContent",
            rows: [
              { cells: [{ type: "tableCell", props: { colspan: 2 }, content: "AB" }] },
              { cells: ["c", "d"] },
            ],
          },
        },
      ],
      editor.document[0].id,
      "after",
    );
    return editor.document.map((b) => b.type).join(",");
  });
  await p.waitForTimeout(400);
  console.log("SONDE blocs après insertion :", inserted, "refus :", await p.locator(".mm-attr-refused").count());
  await expect.poll(() => getMmd(api, page.id)).toContain("AB");
  console.log("SONDE MMD :", JSON.stringify(await getMmd(api, page.id)));

  // Copier le tableau, puis coller ce que BlockNote a mis au presse-papiers.
  const copied = await p.evaluate(() => {
    const editor = window.__mementum!.editor as unknown as {
      setSelection(a: string, b: string): void;
      document: { id: string; type: string }[];
    };
    const table = editor.document.find((b) => b.type === "table")!;
    const first = editor.document[0];
    editor.setSelection(first.id, table.id);
    const data = new DataTransfer();
    const target = document.querySelector(".bn-editor")!;
    target.dispatchEvent(new ClipboardEvent("copy", { clipboardData: data, bubbles: true, cancelable: true }));
    const out: Record<string, string> = {};
    for (const type of data.types) out[type] = data.getData(type);
    return out;
  });
  console.log(
    "SONDE presse-papiers :",
    Object.keys(copied).join(","),
    JSON.stringify((copied["blocknote/html"] ?? copied["text/html"] ?? "").slice(0, 700)),
  );
  // Le curseur à la fin du premier paragraphe : collé dans une cellule, le tableau s'y fondrait.
  await p.evaluate(() => {
    const editor = window.__mementum!.editor as unknown as {
      document: { id: string }[];
      setTextCursorPosition(id: string, at: "end"): void;
    };
    editor.setTextCursorPosition(editor.document[0].id, "end");
  });
  await p.evaluate((copied) => {
    const data = new DataTransfer();
    for (const [type, value] of Object.entries(copied)) data.setData(type, value);
    document
      .querySelector(".bn-editor")!
      .dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, copied);
  await p.waitForTimeout(700);
  const refused = await p.locator(".mm-attr-refused").allTextContents();
  const tables = await p.evaluate(
    () =>
      (window.__mementum!.editor as unknown as { document: { type: string }[] }).document.filter(
        (b) => b.type === "table",
      ).length,
  );
  console.log("SONDE après collage : tableaux =", tables, "refus =", JSON.stringify(refused), "erreurs =", JSON.stringify(errors));
  await p.keyboard.press("Control+Home");
  await p.keyboard.type("vivant ");
  let alive = true;
  try {
    await expect.poll(() => getMmd(api, page.id), { timeout: 8_000 }).toContain("vivant");
  } catch {
    alive = false;
  }
  console.log("SONDE la frappe d'après arrive au serveur :", alive);
  expect(refused).toEqual([]);
  expect(tables).toBe(2);
});
