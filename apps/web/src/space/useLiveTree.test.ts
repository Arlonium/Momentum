// Le canal de l'espace et l'arbre, composés comme dans `App.tsx` : une relecture que le canal
// demande n'annule jamais une lecture en route, un onglet caché attend son retour, et une
// rafale de trames ne fige pas la barre latérale (Ruling R133, R156).
import type { Tree } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getTree } from "../api";
import { renderHook, settle } from "../test/hooks";
import { page } from "../test/pages";
import { useSpaceEvents } from "./useSpaceEvents";
import { useSpaceTree, useTreeReader } from "./useSpaceTree";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTree: vi.fn(),
}));

class FakeSocket {
  static instances: FakeSocket[] = [];
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  constructor(_url: string | URL) {
    FakeSocket.instances.push(this);
  }
  send(): void {}
  close(): void {}
  open(): void {
    this.onopen?.(new Event("open"));
  }
  frame(lsn: number): void {
    this.onmessage?.({ data: JSON.stringify({ lsn }) } as MessageEvent);
  }
}

/** Ce que le serveur sait : la version de l'espace, que l'arbre lu porte (lue avant ses pages). */
const server = { lsn: 0 };
let reads = 0;
let readMs = 0;
const onReturn = vi.fn();
let visibility: DocumentVisibilityState = "visible";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"], now: 1_000_000 });
  visibility = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  FakeSocket.instances = [];
  vi.stubGlobal("WebSocket", FakeSocket);
  server.lsn = 0;
  reads = 0;
  onReturn.mockClear();
  vi.mocked(getTree).mockReset();
  vi.mocked(getTree).mockImplementation(
    (spaceId) =>
      new Promise<Tree>((resolve) => {
        reads += 1;
        const answer: Tree = { spaceId, lsn: server.lsn, nodes: [page("a")] };
        setTimeout(() => resolve(answer), readMs);
      }),
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function composed({ spaceId }: { spaceId: string }) {
  const tree = useSpaceTree();
  const { live } = useSpaceEvents(spaceId, tree.channelChanged, () => tree.knownLsn(spaceId));
  useTreeReader(tree, spaceId, {
    retries: 0,
    live,
    onSessionLost: vi.fn(),
    onGone: vi.fn(),
    onReturn,
  });
  return tree;
}

const pass = (ms: number) => settle(() => vi.advanceTimersByTime(ms));

/** Une écriture de structure : la version monte, le canal la pousse. */
const write = () => settle(() => FakeSocket.instances[0].frame(++server.lsn));

test("slow_tree_read_is_not_thrown_away_by_a_write_each_second", async () => {
  readMs = 1_500;
  const tree = await renderHook(composed, { spaceId: "perso" });
  await settle(() => FakeSocket.instances[0].open());
  for (let second = 0; second < 20; second++) {
    await write();
    await pass(1_000);
  }
  await pass(readMs * 2);
  // L'arbre est affiché, à une version proche de celle du serveur, en un nombre borné de lectures.
  expect(tree.current.loaded).not.toBeNull();
  expect(server.lsn - (tree.current.knownLsn("perso") ?? -100)).toBeLessThanOrEqual(2);
  expect(reads).toBeLessThanOrEqual(14);
});

test("first_frame_before_the_first_read_does_not_throw_it_away", async () => {
  readMs = 1_500;
  server.lsn = 5;
  const tree = await renderHook(composed, { spaceId: "perso" });
  await settle(() => FakeSocket.instances[0].open());
  await settle(() => FakeSocket.instances[0].frame(5));
  await pass(1_000);
  expect(reads).toBe(1);
  await pass(600);
  expect(tree.current.loaded).not.toBeNull();
  expect(tree.current.knownLsn("perso")).toBe(5);
  await pass(10_000);
  expect(reads).toBe(1);
  expect(onReturn).not.toHaveBeenCalled();
});

test("a_burst_of_writes_every_200_ms_still_reloads_about_every_second", async () => {
  readMs = 10;
  const tree = await renderHook(composed, { spaceId: "perso" });
  await settle(() => FakeSocket.instances[0].open());
  await pass(50);
  for (let n = 0; n < 100; n++) {
    await write();
    await pass(200);
  }
  expect(reads).toBeGreaterThanOrEqual(15);
  expect(server.lsn - (tree.current.knownLsn("perso") ?? -100)).toBeLessThanOrEqual(6);
});

test("hidden_tab_waits_for_its_return_to_read_again", async () => {
  readMs = 10;
  const tree = await renderHook(composed, { spaceId: "perso" });
  await settle(() => FakeSocket.instances[0].open());
  await pass(50);
  expect(reads).toBe(1);
  await settle(() => {
    visibility = "hidden";
    document.dispatchEvent(new Event("visibilitychange"));
  });
  for (let n = 0; n < 3; n++) {
    await write();
    await pass(2_000);
  }
  expect(reads).toBe(1);
  // Au retour, une seule lecture, même moins de cinq secondes après la dernière.
  await settle(() => {
    visibility = "visible";
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await pass(50);
  expect(reads).toBe(2);
  expect(tree.current.knownLsn("perso")).toBe(3);
});
