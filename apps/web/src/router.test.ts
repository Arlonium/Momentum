// Les adresses de l'application : un espace, ses réglages, une page.
import { expect, test } from "vitest";
import { pagePath, parseRoute, spacePath, spaceSettingsPath } from "./router";

test("space_routes_are_parsed", () => {
  expect(parseRoute("/")).toEqual({ name: "home" });
  expect(parseRoute("/s/abc")).toEqual({ name: "space", spaceId: "abc" });
  expect(parseRoute("/s/abc/")).toEqual({ name: "space", spaceId: "abc" });
  expect(parseRoute("/s/abc/settings")).toEqual({ name: "spaceSettings", spaceId: "abc" });
  expect(parseRoute("/p/xyz")).toEqual({ name: "page", id: "xyz" });
  // Rien d'autre sous un espace, et pas d'espace sans id.
  expect(parseRoute("/s/abc/autre")).toEqual({ name: "notFound" });
  expect(parseRoute("/s/")).toEqual({ name: "notFound" });
  expect(parseRoute("/s/%E0%A4%A")).toEqual({ name: "notFound" });
});

test("paths_roundtrip_through_the_parser", () => {
  const id = "espace à/part";
  expect(parseRoute(spacePath(id))).toEqual({ name: "space", spaceId: id });
  expect(parseRoute(spaceSettingsPath(id))).toEqual({ name: "spaceSettings", spaceId: id });
  expect(parseRoute(pagePath(id))).toEqual({ name: "page", id });
});
