// Les adresses qu'un bloc suit ou montre : une adresse web (`http:`, `https:`), et rien d'autre
// (plan de la partie B, « Global Constraints », « Client »). Une adresse vient d'ailleurs (MMD,
// un autre client, ce qu'on tape) : tout ce qui n'en est pas une est refusé, jamais deviné.
import { describe, expect, test } from "vitest";
import { hostOf, webUrl } from "./urls";

describe("webUrl", () => {
  test("une adresse http ou https est rendue sous sa forme écrite par le navigateur", () => {
    expect(webUrl("https://mementum.dev")).toBe("https://mementum.dev/");
    expect(webUrl("https://exemple.fr/a")).toBe("https://exemple.fr/a");
    expect(webUrl("http://exemple.fr/a?b=c#d")).toBe("http://exemple.fr/a?b=c#d");
    expect(webUrl("HTTPS://EXEMPLE.FR/A")).toBe("https://exemple.fr/A");
    // Les espaces de bord sont retirées par la lecture d'une URL, comme dans la barre d'adresse.
    expect(webUrl("  https://exemple.fr/a  ")).toBe("https://exemple.fr/a");
  });

  test("un nom de domaine hors de l'ASCII est rendu en punycode, qui ne se confond avec rien", () => {
    expect(webUrl("https://exémple.fr/")).toBe("https://xn--exmple-cva.fr/");
  });

  test("tout autre schéma est refusé", () => {
    for (const raw of [
      "javascript:alert(1)",
      "JavaScript:alert(1)",
      " javascript:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "vbscript:msgbox(1)",
      "file:///etc/passwd",
      "ftp://exemple.fr/",
      "mailto:a@b.fr",
      "blob:https://exemple.fr/1",
    ]) {
      expect(webUrl(raw), raw).toBeNull();
    }
  });

  test("ce qui n'est pas une adresse entière est refusé, jamais complété", () => {
    for (const raw of ["", " ", "//hôte", "//exemple.fr/a", "exemple.fr", "/p/1", "https://", "http:exemple"]) {
      expect(webUrl(raw), JSON.stringify(raw)).toBeNull();
    }
  });

  test("ce qui n'est pas une chaîne est refusé (une prop écrite par un autre client)", () => {
    for (const raw of [undefined, null, 42, true, {}, ["https://exemple.fr/"], { href: "https://exemple.fr/" }]) {
      expect(webUrl(raw), JSON.stringify(raw)).toBeNull();
    }
  });
});

describe("hostOf", () => {
  test("l'hôte d'une adresse web, sans le port par défaut", () => {
    expect(hostOf("https://mementum.dev/")).toBe("mementum.dev");
    expect(hostOf("https://exemple.fr:8443/a")).toBe("exemple.fr:8443");
    expect(hostOf("https://exémple.fr/")).toBe("xn--exmple-cva.fr");
  });

  test("rien pour ce qui n'est pas une adresse web", () => {
    expect(hostOf("javascript:alert(1)")).toBeNull();
    expect(hostOf("")).toBeNull();
  });
});
