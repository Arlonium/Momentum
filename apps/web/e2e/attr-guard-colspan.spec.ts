// Garde des attributs, C2 de la relecture (Ruling R705) : le collage d'un HTML de tableur dont
// une cellule fusionne des colonnes. Sonde du relecteur, gardée.
import { expect, test, type Page } from "@playwright/test";
import type {} from "../src/editor/debug";
import { closeContexts, createPage, defaultAccount, apiAs, getMmd, openPage, unique } from "./helpers";

test.afterEach(closeContexts);

async function paste(p: Page, html: string): Promise<void> {
  await p.evaluate((html) => {
    const target = document.querySelector(".bn-editor")!;
    const data = new DataTransfer();
    data.setData("text/html", html);
    data.setData("text/plain", "collage");
    target.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, html);
}

test("a_cell_merging_up_to_100_columns_is_pasted", async ({ browser }) => {
  test.setTimeout(240_000);
  const api = await apiAs(defaultAccount());
  const refused: number[] = [];
  for (const n of [3, 50, 99, 100]) {
    const html = `<table><tr><td colspan='${n}'>A</td></tr><tr>${"<td>x</td>".repeat(n)}</tr></table>`;
    const page = await createPage(api, { title: `Fusion ${unique()}` });
    const p = await openPage(browser, page.id);
    const errors: string[] = [];
    p.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
    await p.locator(".bn-editor").click();
    await p.keyboard.type("avant");
    await p.keyboard.press("Enter");
    // Les attributs des cellules de chaque transaction envoyée à la vue.
    await p.evaluate(() => {
      const w = window as unknown as { probe: string[] };
      w.probe = [];
      const view = (window.__mementum!.editor as unknown as { prosemirrorView: { dispatch(tr: unknown): void } })
        .prosemirrorView;
      const orig = view.dispatch.bind(view);
      view.dispatch = (tr: unknown) => {
        const steps = (
          tr as {
            steps: {
              slice?: {
                content: {
                  descendants(f: (n: { type: { name: string }; attrs: Record<string, unknown> }) => void): void;
                };
              };
            }[];
          }
        ).steps;
        for (const step of steps) {
          step.slice?.content.descendants((n) => {
            if (n.type.name === "tableCell" || n.type.name === "tableHeader") {
              w.probe.push(
                n.type.name +
                  " " +
                  Object.entries(n.attrs)
                    .map(
                      ([k, v]) =>
                        k + "=" + (Array.isArray(v) ? "[" + v.map((x) => String(x)).join(",") + "]" : typeof v + ":" + String(v)),
                    )
                    .join(" "),
              );
            }
          });
        }
        orig(tr);
      };
    });
    await paste(p, html);
    await p.waitForTimeout(600);
    const alert = (await p.locator(".mm-attr-refused").count()) > 0;
    const mmd = await getMmd(api, page.id);
    console.log(
      "SONDE étapes",
      JSON.stringify((await p.evaluate(() => (window as unknown as { probe: string[] }).probe)).slice(0, 3)),
    );
    console.log(`SONDE fusion ${n} : refus=${alert} erreurs=${JSON.stringify(errors)} MMD=${JSON.stringify(mmd.slice(0, 80))}`);
    if (alert) refused.push(n);
    await p.context().close();
  }
  expect(refused).toEqual([]);
});
