#!/usr/bin/env node
// Vérifie les licences des dépendances npm de production du workspace, d'après
// `pnpm -r licenses list --prod --json`. Échoue sur toute licence hors de la liste de la
// Global Constraints du lot 1 ; GPL-3.0 n'est tolérée que pour @blocknote/xl-multi-column.
//
//   node tools/check-npm-licenses.mjs                    vérifie le workspace
//   node tools/check-npm-licenses.mjs --listing <json>   vérifie une liste déjà produite
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

/** Licences permises (projet sous AGPL-3.0). */
const ALLOWED = new Set([
  "MIT",
  "Apache-2.0",
  "MPL-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "Unicode-3.0",
  "BSL-1.0",
  "0BSD",
  "CC0-1.0",
]);

/** Licences de plus, tolérées pour un seul paquet. */
const EXCEPTIONS = new Map([
  ["@blocknote/xl-multi-column", new Set(["GPL-3.0", "GPL-3.0-only", "GPL-3.0-or-later"])],
]);

function listing() {
  const at = process.argv.indexOf("--listing");
  if (at !== -1) return JSON.parse(readFileSync(process.argv[at + 1], "utf8"));
  // Par le shell : `corepack` est un script `.cmd` sous Windows.
  const out = execSync("corepack pnpm -r licenses list --prod --json", {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  return JSON.parse(out);
}

/** Découpe une expression SPDX : parenthèses, `AND`, `OR`, `WITH` et identifiants. */
function tokens(expression) {
  return expression.replace(/[()]/g, " $& ").split(/\s+/).filter(Boolean);
}

/**
 * L'expression SPDX est-elle satisfaite avec les licences `ok` ? `OR` : une branche
 * suffit ; `AND` : toutes ; `X WITH exception` ne vaut que si X est permise (une exception
 * ajoute des droits). Toute forme illisible (`UNLICENSED`, `SEE LICENSE IN …`) échoue.
 */
function satisfies(expression, ok) {
  const list = tokens(expression);
  let i = 0;
  const fail = () => {
    throw new Error(`expression de licence illisible : « ${expression} »`);
  };
  const atom = () => {
    const token = list[i++];
    if (token === undefined || token === ")" || token === "AND" || token === "OR") fail();
    if (token === "(") {
      const value = or();
      if (list[i++] !== ")") fail();
      return value;
    }
    if (list[i] === "WITH") {
      i += 1;
      if (list[i++] === undefined) fail();
    }
    // `X+` : X ou une version ultérieure.
    return ok.has(token) || (token.endsWith("+") && ok.has(token.slice(0, -1)));
  };
  const and = () => {
    let value = atom();
    while (list[i] === "AND") {
      i += 1;
      value = atom() && value;
    }
    return value;
  };
  const or = () => {
    let value = and();
    while (list[i] === "OR") {
      i += 1;
      value = and() || value;
    }
    return value;
  };
  const value = or();
  if (i !== list.length) fail();
  return value;
}

/** Arrêt clair si la sortie de pnpm n'a plus la forme attendue. */
function unexpected(detail) {
  console.error(
    "Licences npm : sortie de `pnpm licenses list --json` inattendue " +
      `(${detail}). Attendu : un objet { licence: [{ name, versions | version, license }] }. ` +
      "Le format de pnpm a peut-être changé : adapter tools/check-npm-licenses.mjs.",
  );
  process.exit(1);
}

const data = listing();
if (data === null || typeof data !== "object" || Array.isArray(data)) unexpected("pas un objet");

const refused = [];
let count = 0;
for (const [expression, packages] of Object.entries(data)) {
  if (!Array.isArray(packages)) unexpected(`« ${expression} » n'est pas une liste`);
  for (const pkg of packages) {
    if (typeof pkg?.name !== "string") unexpected(`paquet sans nom sous « ${expression} »`);
    count += 1;
    const extra = EXCEPTIONS.get(pkg.name);
    const ok = extra ? new Set([...ALLOWED, ...extra]) : ALLOWED;
    let accepted = false;
    try {
      accepted = satisfies(pkg.license ?? expression, ok);
    } catch {
      accepted = false;
    }
    if (!accepted) {
      const versions = pkg.versions?.join(", ") ?? pkg.version ?? "?";
      refused.push(`${pkg.name}@${versions} : ${pkg.license ?? expression}`);
    }
  }
}

if (count === 0) {
  console.error("Licences npm : aucun paquet de production trouvé (pnpm install a-t-il été lancé ?).");
  process.exit(1);
}
if (refused.length > 0) {
  console.error(`Licences npm : ${refused.length} paquet(s) sous une licence non permise :`);
  for (const line of refused) console.error(`  - ${line}`);
  console.error(`Permises : ${[...ALLOWED].join(", ")} ; GPL-3.0 pour @blocknote/xl-multi-column seulement.`);
  process.exit(1);
}
console.log(
  `Licences npm : ${count} paquets de production, toutes permises ` +
    "(GPL-3.0 tolérée pour @blocknote/xl-multi-column seulement).",
);
