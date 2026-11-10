// La liste de la corbeille d'un espace : sa lecture, ses relectures, et ce qui reste à
// l'écran quand l'une d'elles échoue ou arrive trop tard.
import type { PageNode } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { getTrash } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { page, unauthenticated, unreachable } from "../test/pages";
import { trashedOnLabel, useTrash } from "./useTrash";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTrash: vi.fn(),
}));

/** Les lectures de la corbeille, dans l'ordre : le test tient ou rompt chacune. */
let reads: { spaceId: string; answer: Deferred<PageNode[]> }[] = [];

beforeEach(() => {
  reads = [];
  vi.mocked(getTrash).mockReset();
  vi.mocked(getTrash).mockImplementation((spaceId) => {
    const answer = deferred<PageNode[]>();
    reads.push({ spaceId, answer });
    return answer.promise;
  });
});

type Props = { spaceId: string; version: number };
const hook = ({ spaceId, version }: Props) => useTrash(spaceId, version);
const trashed = (id: string) => page(id, { trashedAt: 1 });

test("trash_is_read_at_first_and_when_its_version_changes", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  expect(reads.map((r) => r.spaceId)).toEqual(["perso"]);
  // Pas encore lue : ni liste, ni erreur.
  expect(trash.current.pages).toBeNull();
  expect(trash.current.error).toBeNull();
  const a = trashed("a");
  await settle(() => reads[0].answer.resolve([a]));
  expect(trash.current.pages).toEqual([a]);

  // Rendue de nouveau sans rien de neuf : pas d'autre lecture.
  await trash.rerender({ spaceId: "perso", version: 0 });
  expect(reads).toHaveLength(1);
  // Une page jetée ou restaurée ailleurs dans l'application : la liste est relue, et celle
  // d'avant reste à l'écran en attendant.
  await trash.rerender({ spaceId: "perso", version: 1 });
  expect(reads).toHaveLength(2);
  expect(trash.current.pages).toEqual([a]);
  const b = trashed("b");
  await settle(() => reads[1].answer.resolve([b, a]));
  expect(trash.current.pages).toEqual([b, a]);
});

test("reload_overtaken_by_a_later_one_is_dropped", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  const a = trashed("a");
  const b = trashed("b");
  await settle(() => reads[0].answer.resolve([a, b]));

  // Deux relectures se suivent (une restauration, puis une page jetée ailleurs) ; la réponse
  // de la première arrive après celle de la seconde.
  let first!: Promise<void>;
  await settle(() => {
    first = trash.current.reload();
  });
  await settle(() => void trash.current.reload());
  expect(reads).toHaveLength(3);
  await settle(() => reads[2].answer.resolve([b]));
  expect(trash.current.pages).toEqual([b]);
  await settle(() => reads[1].answer.resolve([a, b]));
  // La plus ancienne ne remplace pas la plus récente, et son échec n'aurait rien dit non plus.
  expect(trash.current.pages).toEqual([b]);
  expect(trash.current.error).toBeNull();
  // Qui l'attendait n'attend plus.
  await first;
});

test("failed_reload_overtaken_by_a_later_one_says_nothing", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  await settle(() => reads[0].answer.resolve([]));
  await settle(() => void trash.current.reload());
  await settle(() => void trash.current.reload());
  await settle(() => reads[2].answer.resolve([]));
  await settle(() => reads[1].answer.reject(unreachable()));
  expect(trash.current.error).toBeNull();
});

test("failed_first_read_tells_why", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  await settle(() => reads[0].answer.reject(unreachable()));
  expect(trash.current.pages).toBeNull();
  expect(trash.current.error).toBe("Le serveur Mementum ne répond pas.");
  // Réessayée, et réussie : l'erreur part.
  await settle(() => void trash.current.reload());
  await settle(() => reads[1].answer.resolve([]));
  expect(trash.current.pages).toEqual([]);
  expect(trash.current.error).toBeNull();
});

test("failed_reload_keeps_the_list_and_says_so", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  const a = trashed("a");
  await settle(() => reads[0].answer.resolve([a]));

  await settle(() => void trash.current.reload());
  await settle(() => reads[1].answer.reject(unreachable()));
  // La liste déjà lue reste à l'écran, avec le message.
  expect(trash.current.pages).toEqual([a]);
  expect(trash.current.error).toBe("Le serveur Mementum ne répond pas.");

  // La relecture suivante qui réussit l'efface.
  await settle(() => void trash.current.reload());
  await settle(() => reads[2].answer.resolve([]));
  expect(trash.current.pages).toEqual([]);
  expect(trash.current.error).toBeNull();
});

test("lost_session_says_nothing_and_keeps_the_list", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  const a = trashed("a");
  await settle(() => reads[0].answer.resolve([a]));
  await settle(() => void trash.current.reload());
  // La connexion est demandée par-dessus l'application : pas de message de plus.
  await settle(() => reads[1].answer.reject(unauthenticated()));
  expect(trash.current.pages).toEqual([a]);
  expect(trash.current.error).toBeNull();
});

test("answer_after_the_view_is_gone_is_dropped", async () => {
  const trash = await renderHook(hook, { spaceId: "perso", version: 0 });
  const before = trash.current;
  await trash.unmount();
  await settle(() => reads[0].answer.resolve([trashed("a")]));
  expect(trash.current).toBe(before);
});

test("trashed_on_carries_the_year_only_when_it_is_not_this_year", () => {
  // Midi, temps universel, en milieu de mois : le même jour sous tous les fuseaux.
  const now = Date.UTC(2026, 10, 10, 12);
  const thisYear = trashedOnLabel(Date.UTC(2026, 2, 15, 12), now);
  expect(thisYear).toMatch(/^15 mars à \d{2}:\d{2}$/);
  const lastYear = trashedOnLabel(Date.UTC(2025, 2, 15, 12), now);
  expect(lastYear).toMatch(/^15 mars 2025 à \d{2}:\d{2}$/);
  // Même mois, même jour, une autre année : c'est l'année qui les distingue.
  expect(lastYear).not.toBe(thisYear);
});
