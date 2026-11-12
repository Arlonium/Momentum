// Le canal de l'espace (`GET /events/{spaceId}`, R133) : le serveur y pousse la version de
// l'espace (`{"lsn":N}`), le client relit l'arbre quand elle dépasse celle de l'arbre affiché
// (R141). La WebSocket est simulée ; le client n'y envoie jamais rien.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { renderHook, settle } from "../test/hooks";
import { EVENTS_DEBOUNCE_MS, useSpaceEvents } from "./useSpaceEvents";

class FakeSocket {
  static instances: FakeSocket[] = [];
  readonly url: string;
  readyState = 0;
  closed = false;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  sent: unknown[] = [];

  constructor(url: string | URL) {
    this.url = String(url);
    FakeSocket.instances.push(this);
  }

  send(data: unknown): void {
    this.sent.push(data);
  }

  close(): void {
    this.closed = true;
    this.readyState = 3;
  }

  open(): void {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }

  frame(lsn: number): void {
    this.onmessage?.({ data: JSON.stringify({ lsn }) } as MessageEvent);
  }

  /** Le serveur ferme, ou refuse la poignée de main (`401`, `404`). */
  drop(): void {
    this.readyState = 3;
    this.onclose?.(new Event("close") as CloseEvent);
  }
}

const onChange = vi.fn();
let known: number | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  FakeSocket.instances = [];
  vi.stubGlobal("WebSocket", FakeSocket);
  onChange.mockClear();
  known = null;
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

type Props = { spaceId: string | null };
const hook = ({ spaceId }: Props) => useSpaceEvents(spaceId, onChange, () => known);
const advance = (ms: number) => settle(() => vi.advanceTimersByTime(ms));

test("a_higher_version_reloads_the_tree_once", async () => {
  known = 4;
  await renderHook(hook, { spaceId: "perso" });
  const socket = FakeSocket.instances[0];
  expect(socket.url).toMatch(/\/events\/perso$/);
  await settle(() => socket.open());
  await settle(() => socket.frame(5));
  await advance(40);
  await settle(() => socket.frame(6));
  await advance(40);
  await settle(() => socket.frame(7));
  // Trois trames en 100 ms : une seule relecture, 250 ms après la dernière.
  await advance(EVENTS_DEBOUNCE_MS - 1);
  expect(onChange).not.toHaveBeenCalled();
  await advance(1);
  expect(onChange).toHaveBeenCalledTimes(1);
  expect(socket.sent).toEqual([]);
});

test("the_first_frame_reloads_nothing_when_the_tree_is_as_recent", async () => {
  known = 5;
  await renderHook(hook, { spaceId: "perso" });
  const socket = FakeSocket.instances[0];
  await settle(() => socket.open());
  await settle(() => socket.frame(5));
  await advance(EVENTS_DEBOUNCE_MS * 4);
  expect(onChange).not.toHaveBeenCalled();
});

test("a_lower_or_equal_version_is_ignored", async () => {
  known = 9;
  await renderHook(hook, { spaceId: "perso" });
  const socket = FakeSocket.instances[0];
  await settle(() => socket.open());
  await settle(() => socket.frame(9));
  await settle(() => socket.frame(3));
  await advance(EVENTS_DEBOUNCE_MS * 4);
  expect(onChange).not.toHaveBeenCalled();
  // Une trame que la relecture a déjà rattrapée n'en demande pas une seconde.
  await settle(() => socket.frame(10));
  known = 10;
  await advance(EVENTS_DEBOUNCE_MS);
  expect(onChange).not.toHaveBeenCalled();
});

test("a_closed_channel_comes_back", async () => {
  await renderHook(hook, { spaceId: "perso" });
  await settle(() => FakeSocket.instances[0].open());
  await settle(() => FakeSocket.instances[0].frame(1));
  await settle(() => FakeSocket.instances[0].drop());
  expect(FakeSocket.instances).toHaveLength(1);
  // Une seconde, puis le double à chaque échec (sans trame entre-temps), trente au plus.
  await advance(999);
  expect(FakeSocket.instances).toHaveLength(1);
  await advance(1);
  expect(FakeSocket.instances).toHaveLength(2);
  await settle(() => FakeSocket.instances[1].drop());
  await advance(1_999);
  expect(FakeSocket.instances).toHaveLength(2);
  await advance(1);
  expect(FakeSocket.instances).toHaveLength(3);
  let delay = 4_000;
  for (let n = 3; n < 8; n += 1) {
    await settle(() => FakeSocket.instances[n - 1].drop());
    await advance(Math.min(delay, 30_000) - 1);
    expect(FakeSocket.instances).toHaveLength(n);
    await advance(1);
    expect(FakeSocket.instances).toHaveLength(n + 1);
    delay *= 2;
  }
  // Plafonné à trente secondes.
  await settle(() => FakeSocket.instances[7].drop());
  await advance(29_999);
  expect(FakeSocket.instances).toHaveLength(8);
  await advance(1);
  expect(FakeSocket.instances).toHaveLength(9);
  // Une trame reçue remet le délai à une seconde.
  await settle(() => FakeSocket.instances[8].open());
  await settle(() => FakeSocket.instances[8].frame(2));
  await settle(() => FakeSocket.instances[8].drop());
  await advance(1_000);
  expect(FakeSocket.instances).toHaveLength(10);
});

test("live_is_false_while_the_channel_is_down", async () => {
  const events = await renderHook(hook, { spaceId: "perso" });
  expect(events.current.live).toBe(false);
  await settle(() => FakeSocket.instances[0].open());
  expect(events.current.live).toBe(true);
  await settle(() => FakeSocket.instances[0].drop());
  expect(events.current.live).toBe(false);
  await advance(1_000);
  await settle(() => FakeSocket.instances[1].open());
  expect(events.current.live).toBe(true);
  // Un refus à la poignée de main (jamais ouvert) : le canal reste tombé.
  await settle(() => FakeSocket.instances[1].drop());
  await advance(2_000);
  await settle(() => FakeSocket.instances[2].drop());
  expect(events.current.live).toBe(false);
});

test("the_channel_closes_with_the_hook_and_with_the_space", async () => {
  known = 0;
  const events = await renderHook(hook, { spaceId: "perso" });
  const first = FakeSocket.instances[0];
  await settle(() => first.open());
  await settle(() => first.frame(3));
  await events.rerender({ spaceId: "autre" });
  expect(first.closed).toBe(true);
  expect(FakeSocket.instances[1].url).toMatch(/\/events\/autre$/);
  // Rien de la première connexion ne parle plus : ni la trame en attente, ni la reconnexion.
  await advance(EVENTS_DEBOUNCE_MS * 2);
  expect(onChange).not.toHaveBeenCalled();
  await settle(() => FakeSocket.instances[1].open());
  await settle(() => FakeSocket.instances[1].frame(3));
  const second = FakeSocket.instances[1];
  await events.unmount();
  expect(second.closed).toBe(true);
  await advance(60_000);
  expect(onChange).not.toHaveBeenCalled();
  expect(FakeSocket.instances).toHaveLength(2);
});

test("no_channel_without_a_space", async () => {
  const events = await renderHook(hook, { spaceId: null });
  expect(FakeSocket.instances).toEqual([]);
  expect(events.current.live).toBe(false);
});
