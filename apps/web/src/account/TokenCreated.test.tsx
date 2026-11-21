// Le jeton tout juste créé, et les commandes qui branchent Claude Code (lot 5, Task C1, R1576) : une
// par shell, en deux lignes, la première lit le jeton dans `MEMENTUM_TOKEN` sans l'afficher, la
// seconde le nomme par cette variable, jamais par son texte (l'historique du shell le garderait).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import "../test/hooks";
import { TokenCreated } from "./TokenCreated";

/** Un jeton de la forme du serveur (`mmt_` et 43 caractères), fabriqué ici : jamais un vrai. */
const TOKEN = "mmt_TEST0000000000000000000000000000000000000ab";
const ORIGIN = "https://notes.exemple.fr";

let host: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<TokenCreated token={TOKEN} origin={ORIGIN} />));
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const shells = () => [...host.querySelectorAll<HTMLInputElement>('input[type="radio"][name$="shell"]')];
const labelOf = (input: HTMLInputElement) => input.closest("label")?.textContent?.trim() ?? "";
const choose = (label: string) =>
  act(async () => {
    shells()
      .find((input) => labelOf(input) === label)!
      .click();
  });
/** Les lignes de commande montrées, dans l'ordre. */
const commands = () => [...host.querySelectorAll<HTMLElement>("code.mm-token-command")].map((c) => c.textContent ?? "");

test("the_command_to_paste_never_carries_the_token", async () => {
  expect(shells().map(labelOf)).toEqual(["bash ou zsh", "PowerShell", "cmd"]);
  for (const label of ["bash ou zsh", "PowerShell", "cmd"]) {
    await choose(label);
    const lines = commands();
    expect(lines.length, label).toBeGreaterThanOrEqual(2);
    for (const line of lines) {
      // Ni le jeton, ni un morceau de lui : seulement le nom de la variable.
      expect(line, label).not.toContain(TOKEN);
      expect(line, label).not.toContain(TOKEN.slice(4, 20));
      expect(line, label).toContain("MEMENTUM_TOKEN");
    }
    // Une ligne lit le jeton dans la variable ; une autre branche Mementum, à l'adresse que le
    // navigateur connaît, en nommant la variable dans l'en-tête.
    expect(lines.some((line) => /MEMENTUM_TOKEN\s*=|read -rs MEMENTUM_TOKEN/.test(line)), label).toBe(true);
    const add = lines.find((line) => line.startsWith("claude mcp add"));
    expect(add, label).toContain("--transport http mementum");
    expect(add, label).toContain(`${ORIGIN}/mcp`);
    expect(add, label).toContain("Authorization: Bearer ${MEMENTUM_TOKEN}");
  }
  // Le jeton n'est montré qu'à un endroit : son champ, à copier.
  const fields = [...host.querySelectorAll("input")].filter((input) => input.value === TOKEN);
  expect(fields).toHaveLength(1);
  expect(fields[0].getAttribute("aria-label")).toBe("Jeton d'API");
  expect(host.textContent).not.toContain(TOKEN);
});

test("each_shell_gets_its_own_quotes", async () => {
  // bash et zsh : l'en-tête entre apostrophes, que le shell ne développe pas ; la saisie masquée.
  await choose("bash ou zsh");
  let lines = commands();
  expect(lines[0]).toBe("read -rs MEMENTUM_TOKEN && export MEMENTUM_TOKEN");
  expect(lines[1]).toBe(
    `claude mcp add --transport http mementum '${ORIGIN}/mcp' --header 'Authorization: Bearer \${MEMENTUM_TOKEN}'`,
  );

  // PowerShell : entre apostrophes aussi (entre guillemets, il développerait sa propre variable,
  // vide) ; `-MaskInput` demande PowerShell 7.1, et la ligne de Windows PowerShell 5.1 est à côté.
  await choose("PowerShell");
  lines = commands();
  expect(lines[0]).toBe("$env:MEMENTUM_TOKEN = Read-Host -MaskInput");
  expect(lines).toContain("$env:MEMENTUM_TOKEN = [Net.NetworkCredential]::new('', (Read-Host -AsSecureString)).Password");
  expect(lines).toContain(
    `claude mcp add --transport http mementum '${ORIGIN}/mcp' --header 'Authorization: Bearer \${MEMENTUM_TOKEN}'`,
  );
  expect(host.textContent).toContain("Windows PowerShell 5.1");

  // cmd : entre guillemets (une apostrophe n'y groupe rien) ; pas de saisie masquée, et l'écran le dit.
  await choose("cmd");
  lines = commands();
  expect(lines[0]).toBe("set /p MEMENTUM_TOKEN=Jeton : ");
  expect(lines[1]).toBe(
    `claude mcp add --transport http mementum "${ORIGIN}/mcp" --header "Authorization: Bearer \${MEMENTUM_TOKEN}"`,
  );
  expect(host.textContent).toContain("s'affiche pendant que tu le tapes");

  // Dans tous les shells : la variable se pose, dans le shell qui lance Claude Code, à chaque lancement.
  expect(host.textContent).toContain("à chaque lancement de Claude Code");
});
