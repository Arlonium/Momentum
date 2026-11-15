// « Favoris », en haut de la barre latérale, ne bouge pas sous le pointeur (R296, R298, R411) :
// tant qu'il est dans la barre, ce qu'une relecture change à la liste attend qu'il en sorte ; ce
// que la personne y fait elle-même se montre tout de suite.
import type { PageNode } from "@mementum/protocol";
import { expect, test } from "vitest";
import { renderHook, settle } from "../test/hooks";
import { page } from "../test/pages";
import { heldPages, useHeldPages } from "./heldPages";

const ids = (pages: PageNode[]) => pages.map((p) => p.id);
const NONE: ReadonlySet<string> = new Set();

test("held_favorites_keep_their_size_and_order_and_follow_titles", () => {
  const [a, b, c] = [page("a"), page("b"), page("c")];
  // Relu : « a » est partie (corbeille, accès retiré), « c » est arrivée, « b » renommée.
  const renamed = page("b", { title: "Renommée" });
  const shown = heldPages([a, b], [renamed, c], NONE);
  // La même taille, le même ordre : « a » reste à sa place, « c » attend ; le titre suit.
  expect(ids(shown)).toEqual(["a", "b"]);
  expect(shown[0]).toBe(a);
  expect(shown[1]).toBe(renamed);
});

test("own_gestures_show_at_once_while_held", () => {
  const [a, b, c, d] = [page("a"), page("b"), page("c"), page("d")];
  // Posé d'ici : à la fin, tout de suite ; une arrivée d'ailleurs attend toujours.
  expect(ids(heldPages([a, b], [a, b, c, d], new Set(["d"])))).toEqual(["a", "b", "d"]);
  // Retiré d'ici : il part tout de suite ; un départ d'ailleurs attend toujours.
  expect(ids(heldPages([a, b, c], [c], new Set(["a"])))).toEqual(["b", "c"]);
  // Retiré puis reposé d'ici : à sa place d'avant.
  expect(ids(heldPages([a, b], [b, a], new Set(["a"])))).toEqual(["a", "b"]);
});

type Props = {
  spaceId: string;
  incoming: PageNode[];
  held: boolean;
  toggles?: ReadonlyMap<string, number>;
};
const NO_TOGGLES: ReadonlyMap<string, number> = new Map();
const hook = ({ spaceId, incoming, held, toggles = NO_TOGGLES }: Props) =>
  useHeldPages(spaceId, incoming, held, toggles);

test("favorites_are_held_while_the_pointer_is_in_and_thaw_once_it_leaves", async () => {
  const [a, b, c] = [page("a"), page("b"), page("c")];
  const rendered = await renderHook(hook, { spaceId: "s", incoming: [a, b], held: false });
  // Sans pointeur : ce qui arrive se montre.
  await rendered.rerender({ spaceId: "s", incoming: [a, b, c], held: false });
  expect(ids(rendered.current)).toEqual(["a", "b", "c"]);
  // Le pointeur entre : la liste montrée est gardée.
  await rendered.rerender({ spaceId: "s", incoming: [a, b, c], held: true });
  await rendered.rerender({ spaceId: "s", incoming: [b], held: true });
  expect(ids(rendered.current)).toEqual(["a", "b", "c"]);
  // Il sort : la liste reçue.
  await rendered.rerender({ spaceId: "s", incoming: [b], held: false });
  expect(ids(rendered.current)).toEqual(["b"]);
  // De nouveau dedans, ce qui était montré à l'entrée est gardé, pas une liste d'avant.
  await rendered.rerender({ spaceId: "s", incoming: [b], held: true });
  await rendered.rerender({ spaceId: "s", incoming: [b, c], held: true });
  expect(ids(rendered.current)).toEqual(["b"]);
  await settle();
});

test("the_favorites_of_another_space_are_not_held_in_this_one", async () => {
  // Le sélecteur d'espace est dans la barre : on change d'espace le pointeur dedans. La liste de
  // l'espace d'avant n'est pas gardée dans le nouveau ; celle du nouveau l'est, telle qu'on l'y a
  // vue en arrivant.
  const [a, x, y] = [page("a"), page("x", { spaceId: "pro" }), page("y", { spaceId: "pro" })];
  const rendered = await renderHook(hook, { spaceId: "s", incoming: [a], held: true });
  await rendered.rerender({ spaceId: "pro", incoming: [x], held: true });
  expect(ids(rendered.current)).toEqual(["x"]);
  await rendered.rerender({ spaceId: "pro", incoming: [x, y], held: true });
  expect(ids(rendered.current)).toEqual(["x"]);
  await rendered.rerender({ spaceId: "pro", incoming: [x, y], held: false });
  expect(ids(rendered.current)).toEqual(["x", "y"]);
});

test("gestures_made_here_since_the_hold_began_show_at_once", async () => {
  // Les gestes de l'onglet (menu d'une ligne ou de la page), par leur rang : ceux d'après le début
  // du gel se voient tout de suite ; ceux d'avant n'exemptent rien de ce qu'une relecture change.
  const [a, b, c] = [page("a"), page("b"), page("c")];
  const before = new Map([["a", 1]]);
  const rendered = await renderHook(hook, { spaceId: "s", incoming: [a], held: true, toggles: before });
  // « a », posé avant le gel, est retiré ailleurs : le départ attend.
  await rendered.rerender({ spaceId: "s", incoming: [], held: true, toggles: before });
  expect(ids(rendered.current)).toEqual(["a"]);
  // « b », posé d'ici pendant le gel : il se voit ; « c », arrivé d'ailleurs, attend.
  const after = new Map([["a", 1], ["b", 2]]);
  await rendered.rerender({ spaceId: "s", incoming: [b, c], held: true, toggles: after });
  expect(ids(rendered.current)).toEqual(["a", "b"]);
  // « a », retiré d'ici à son tour : il part.
  const again = new Map([["a", 3], ["b", 2]]);
  await rendered.rerender({ spaceId: "s", incoming: [b, c], held: true, toggles: again });
  expect(ids(rendered.current)).toEqual(["b"]);
  await rendered.rerender({ spaceId: "s", incoming: [b, c], held: false, toggles: again });
  expect(ids(rendered.current)).toEqual(["b", "c"]);
  // Le gel suivant repart du rang atteint : un geste d'avant lui n'exempte plus rien.
  await rendered.rerender({ spaceId: "s", incoming: [b, c], held: true, toggles: again });
  await rendered.rerender({ spaceId: "s", incoming: [c], held: true, toggles: again });
  expect(ids(rendered.current)).toEqual(["b", "c"]);
});
