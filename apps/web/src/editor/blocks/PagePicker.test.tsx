// Le sélecteur de page (Task C3) : un champ et la liste des pages que le compte voit dans l'espace
// affiché (`pageDirectory.ts`), huit au plus, au clavier et à la souris. Une page qui n'est pas dans
// l'arbre n'est jamais proposée (R251, preuve 4 du plan).
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import "../../test/hooks";
import { page } from "../../test/pages";
import { fill, press } from "../../test/richEditor";
import { publishPages, type FoundPage } from "../../space/pageDirectory";
import { PagePicker } from "./PagePicker";

const roots: { root: Root; host: HTMLElement }[] = [];
afterEach(async () => {
  for (const { root, host } of roots.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  publishPages(null, null);
});

/** Le sélecteur rendu, son champ tenu comme le tient sa fenêtre ; rend ce qui a été choisi. */
async function mountPicker({ query = "", disabled = false } = {}) {
  const picked: FoundPage[] = [];
  function Host() {
    const [text, setText] = useState(query);
    return <PagePicker query={text} onQuery={setText} onPick={(p) => picked.push(p)} disabled={disabled} />;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push({ root, host });
  await act(async () => root.render(<Host />));
  const field = host.querySelector<HTMLInputElement>('input[role="combobox"]')!;
  return {
    host,
    field,
    picked,
    options: () => [...host.querySelectorAll<HTMLElement>('[role="listbox"] [role="option"]')],
    titles: () => [...host.querySelectorAll('[role="option"]')].map((o) => o.textContent),
    active: () => {
      const id = field.getAttribute("aria-activedescendant");
      return id === null ? null : document.getElementById(id)?.textContent ?? null;
    },
  };
}

const ID = (n: number) => `0190f5a0-0000-7000-8000-${String(n).padStart(12, "0")}`;

test("page_picker_offers_only_published_pages", async () => {
  publishPages("space", [
    page(ID(1), { title: "Plan" }),
    page(ID(2), { title: "Notes" }),
    page(ID(3), { title: "Planning" }),
    page(ID(4), { title: "Plan caché", trashedAt: 5 }),
  ]);
  const p = await mountPicker();
  expect(p.field.labels?.[0]?.textContent).toBe("Chercher une page");
  // Une requête vide : les pages de l'arbre, hors corbeille.
  expect(p.titles()).toEqual(["Plan", "Notes", "Planning"]);
  await act(async () => fill(p.field, "plan"));
  expect(p.titles()).toEqual(["Plan", "Planning"]);
  // Une page que l'arbre ne porte pas (qu'on ne voit pas) n'est jamais proposée, même par son titre.
  await act(async () => fill(p.field, "secret"));
  expect(p.titles()).toEqual([]);
  expect(p.host.querySelector('[role="status"]')?.textContent).toBe("Aucune page ne porte ce titre.");
  // Témoin : publiée, elle l'est.
  await act(async () => publishPages("space", [page(ID(1), { title: "Plan" }), page(ID(9), { title: "Secret" })]));
  expect(p.titles()).toEqual(["Secret"]);
});

test("page_picker_offers_eight_pages_at_most", async () => {
  publishPages(
    "space",
    Array.from({ length: 12 }, (_, i) => page(ID(i + 1), { title: `Page ${i + 1}` })),
  );
  const p = await mountPicker({ query: "page" });
  expect(p.titles()).toEqual(["Page 1", "Page 2", "Page 3", "Page 4", "Page 5", "Page 6", "Page 7", "Page 8"]);
  expect(p.host.querySelector('[role="listbox"]')?.tagName).toBe("UL");
  expect(p.options().every((o) => o.tagName === "LI")).toBe(true);
});

test("page_picker_is_used_from_the_keyboard", async () => {
  publishPages("space", [page(ID(1), { title: "Plan" }), page(ID(2), { title: "Notes" }), page(ID(3), { title: "Nuit" })]);
  const p = await mountPicker();
  expect(p.field.getAttribute("aria-controls")).toBe(p.host.querySelector('[role="listbox"]')!.id);
  expect(p.active()).toBe("Plan");
  await act(async () => void press(p.field, "ArrowDown"));
  expect(p.active()).toBe("Notes");
  await act(async () => void press(p.field, "ArrowDown"));
  await act(async () => void press(p.field, "ArrowDown"));
  // Au bout, on y reste ; flèche haut remonte.
  expect(p.active()).toBe("Nuit");
  await act(async () => void press(p.field, "ArrowUp"));
  expect(p.active()).toBe("Notes");
  expect(p.options().map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "true", "false"]);
  const enter = press(p.field, "Enter");
  expect(enter.defaultPrevented).toBe(true);
  expect(p.picked).toEqual([{ id: ID(2), title: "Notes" }]);
  // La requête change : la première page redevient l'active.
  await act(async () => fill(p.field, "n"));
  expect(p.active()).toBe("Notes");
  // Un clic choisit aussi.
  await act(async () => p.options()[1].click());
  expect(p.picked.at(-1)).toEqual({ id: ID(3), title: "Nuit" });
});

test("page_picker_follows_the_tree", async () => {
  publishPages("space", [page(ID(1), { title: "Plan" })]);
  const p = await mountPicker();
  expect(p.titles()).toEqual(["Plan"]);
  await act(async () => publishPages("space", [page(ID(1), { title: "Plan 2027" }), page(ID(2), { title: "Notes" })]));
  expect(p.titles()).toEqual(["Plan 2027", "Notes"]);
  // Quitter l'espace : plus rien.
  await act(async () => publishPages(null, null));
  expect(p.titles()).toEqual([]);
  expect(p.host.querySelector('[role="status"]')?.textContent).toBe("Aucune page à proposer.");
});

test("a_disabled_picker_picks_nothing", async () => {
  publishPages("space", [page(ID(1), { title: "Plan" })]);
  const p = await mountPicker({ disabled: true });
  press(p.field, "Enter");
  await act(async () => p.options()[0].click());
  expect(p.picked).toEqual([]);
  expect(p.options()[0].getAttribute("aria-disabled")).toBe("true");
});

test("page_titles_are_shown_as_text", async () => {
  publishPages("space", [page(ID(1), { title: '<img src=x onerror="alert(1)"> ‮Été' })]);
  const p = await mountPicker();
  expect(p.titles()).toEqual(['<img src=x onerror="alert(1)"> ‮Été']);
  expect(p.host.querySelector("img")).toBeNull();
  const title = p.options()[0].querySelector("[dir='auto']");
  expect(title?.textContent).toBe('<img src=x onerror="alert(1)"> ‮Été');
  // Entier dans l'infobulle, entre U+2068 et U+2069.
  expect(p.options()[0].getAttribute("title")).toBe('⁨<img src=x onerror="alert(1)"> ‮Été⁩');
  expect(p.host.querySelector("[style]")).toBeNull();
});
