// Les relectures groupées (`rereads.ts`), que partagent les commentaires et l'historique : une
// rafale de demandes ne fait qu'une lecture, une lecture en route n'est jamais annulée, une
// demande faite pendant elle est servie une fois, à sa fin, et rien ne part après l'arrêt.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  groupedDelay,
  REREAD_DEBOUNCE_MS,
  REREAD_MAX_WAIT_MS,
  scheduleRereads,
  type Rereads,
} from "./rereads";
import { deferred, type Deferred } from "./test/hooks";

/** Les lectures, dans l'ordre : le test tient ou rompt chacune ; `true` : à refaire. */
let reads: Deferred<boolean | void>[] = [];
let rereads: Rereads;

beforeEach(() => {
  vi.useFakeTimers();
  reads = [];
  rereads = scheduleRereads(() => {
    const answer = deferred<boolean | void>();
    reads.push(answer);
    return answer.promise;
  });
});

afterEach(() => {
  rereads.stop();
  vi.useRealTimers();
});

/** Avance l'horloge, et laisse passer les promesses tenues. */
async function advance(ms: number): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
}

test("grouped_delay_is_the_debounce_bounded_by_the_max_wait", () => {
  expect(REREAD_DEBOUNCE_MS).toBe(250);
  expect(REREAD_MAX_WAIT_MS).toBe(1_000);
  // Première demande : l'anti-rebond entier.
  expect(groupedDelay(10_000, 10_000)).toBe(250);
  // Une demande 900 ms après la première : il ne reste que 100 ms avant la borne.
  expect(groupedDelay(10_000, 10_900)).toBe(100);
  // La borne passée : tout de suite, jamais un délai négatif.
  expect(groupedDelay(10_000, 12_000)).toBe(0);
});

test("an_immediate_request_reads_on_the_next_turn", async () => {
  rereads.request(true);
  expect(reads).toHaveLength(0);
  await advance(0);
  expect(reads).toHaveLength(1);
});

test("a_burst_of_requests_reads_once_after_the_last", async () => {
  for (let i = 0; i < 3; i += 1) {
    rereads.request(false);
    await advance(REREAD_DEBOUNCE_MS - 50);
  }
  expect(reads).toHaveLength(0);
  await advance(50);
  expect(reads).toHaveLength(1);
});

test("a_steady_stream_of_requests_reads_within_the_max_wait", async () => {
  // Une demande toutes les 200 ms : l'anti-rebond seul ne lirait jamais.
  for (let elapsed = 0; elapsed < REREAD_MAX_WAIT_MS; elapsed += 200) {
    rereads.request(false);
    await advance(200);
  }
  expect(reads).toHaveLength(1);
});

test("an_immediate_request_is_not_pushed_back_by_grouped_ones", async () => {
  rereads.request(true);
  rereads.request(false);
  await advance(0);
  expect(reads).toHaveLength(1);
  await settled(0);
  await advance(REREAD_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
});

test("a_request_during_a_read_is_served_once_at_its_end", async () => {
  rereads.request(true);
  await advance(0);
  expect(reads).toHaveLength(1);
  // Trois demandes pendant la lecture : elle n'est pas jetée, une seule relecture suit.
  rereads.request(false);
  rereads.request(true);
  rereads.request(false);
  await advance(REREAD_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  await settled(0);
  expect(reads).toHaveLength(2);
  await settled(1);
  await advance(REREAD_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("a_read_that_asks_to_be_done_again_is_done_again", async () => {
  rereads.request(true);
  await advance(0);
  // La réponse peut dater d'avant une écriture d'ici : la lecture le dit, et elle est refaite.
  await settled(0, true);
  expect(reads).toHaveLength(2);
  await settled(1);
  await advance(REREAD_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("a_failed_read_does_not_loop", async () => {
  rereads.request(true);
  await advance(0);
  reads[0].reject(new Error("réseau"));
  await advance(60_000);
  expect(reads).toHaveLength(1);
  // La demande suivante relit.
  rereads.request(true);
  await advance(0);
  expect(reads).toHaveLength(2);
});

test("nothing_is_read_after_stop", async () => {
  rereads.request(false);
  rereads.stop();
  expect(rereads.stopped).toBe(true);
  await advance(REREAD_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(0);
  rereads.request(true);
  await advance(0);
  expect(reads).toHaveLength(0);
});

test("a_read_in_flight_at_stop_is_not_followed_by_another", async () => {
  const other = scheduleRereads(() => {
    const answer = deferred<boolean | void>();
    reads.push(answer);
    return answer.promise;
  });
  other.request(true);
  await advance(0);
  other.request(false);
  other.stop();
  await settled(0, true);
  await advance(REREAD_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
});

/** Tient la lecture `index` (avec `again` : à refaire), et laisse passer ce qui s'ensuit. */
async function settled(index: number, again?: boolean): Promise<void> {
  reads[index].resolve(again);
  await advance(0);
}
