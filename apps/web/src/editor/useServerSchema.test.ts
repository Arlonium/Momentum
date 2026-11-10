// La version de schéma du serveur : lue au chargement, relue avant chaque connexion (R35).
import type { Info } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { getInfo } from "../api";
import { renderHook, settle } from "../test/hooks";
import { unreachable } from "../test/pages";
import { useServerSchema } from "./useServerSchema";
import { CLIENT_SCHEMA_VERSION } from "./version";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getInfo: vi.fn(),
}));

const info = (schemaVersion: number): Info => ({
  version: "test",
  schemaVersion,
  signup: "invite",
  setup: false,
});

beforeEach(() => {
  vi.mocked(getInfo).mockReset();
});

test("same_schema_loads_writable", async () => {
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION));
  const schema = await renderHook(useServerSchema, undefined);
  await settle();
  expect(schema.current).toMatchObject({ loaded: true, error: null, stale: false });
  expect(schema.current.isStale()).toBe(false);
  expect(await schema.current.check()).toBe(true);
});

test("other_schema_is_stale_for_good", async () => {
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION + 1));
  const schema = await renderHook(useServerSchema, undefined);
  await settle();
  expect(schema.current).toMatchObject({ loaded: true, stale: true });
  // Le serveur revenu au schéma du client : l'onglet reste en lecture seule (ses documents
  // ont été remplacés, R35).
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION));
  let same = true;
  await settle(async () => {
    same = await schema.current.check();
  });
  expect(same).toBe(false);
  expect(schema.current.isStale()).toBe(true);
});

test("schema_changed_while_open_is_seen_at_the_next_check", async () => {
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION));
  const schema = await renderHook(useServerSchema, undefined);
  await settle();
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION + 1));
  let same = true;
  await settle(async () => {
    same = await schema.current.check();
  });
  expect(same).toBe(false);
  expect(schema.current.stale).toBe(true);
});

test("unreachable_server_at_load_is_an_error", async () => {
  vi.mocked(getInfo).mockRejectedValue(unreachable());
  const schema = await renderHook(useServerSchema, undefined);
  await settle();
  expect(schema.current).toMatchObject({
    loaded: false,
    error: "Le serveur Mementum ne répond pas.",
  });
});
