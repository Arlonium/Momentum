// Le jeton tout juste créé, et les commandes qui branchent Claude Code (lot 5, Task C1, R1576, R1462) :
// une par shell, la première lit le jeton dans `MEMENTUM_TOKEN` avec une invite, sans l'afficher, la
// dernière le nomme par cette variable, jamais par son texte (l'historique du shell le garderait) ; et
// les gestes dans l'ordre où le presse-papiers les sert : la ligne qui attend, puis le jeton, puis la
// ligne qui branche.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import "../test/hooks";
import { commandsFor, TokenCreated, type Shell } from "./TokenCreated";

/** Un jeton de la forme du serveur (`mmt_` et 43 caractères), fabriqué ici : jamais un vrai. */
const TOKEN = "mmt_TEST0000000000000000000000000000000000000ab";
const ORIGIN = "https://notes.exemple.fr";

let host: HTMLDivElement;
let root: Root;
/** Le système que le navigateur annonce (`navigator.platform`), pour le shell proposé d'abord. */
let platform = "Linux x86_64";

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  platform = "Linux x86_64";
  Object.defineProperty(navigator, "platform", { configurable: true, get: () => platform });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const rendered = (origin = ORIGIN) => act(async () => root.render(<TokenCreated token={TOKEN} origin={origin} />));
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
const SHELL_LABELS = ["bash ou zsh", "PowerShell", "cmd"];

test("the_command_to_paste_never_carries_the_token", async () => {
  await rendered();
  expect(shells().map(labelOf)).toEqual(SHELL_LABELS);
  for (const label of SHELL_LABELS) {
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
    // Le jeton n'est montré qu'à un endroit : son champ, à copier.
    const fields = [...host.querySelectorAll("input")].filter((input) => input.value === TOKEN);
    expect(fields, label).toHaveLength(1);
    expect(fields[0].getAttribute("aria-label")).toBe("Jeton d'API");
    expect(host.textContent).not.toContain(TOKEN);
  }
});

test("each_shell_gets_its_own_quotes", async () => {
  await rendered();
  // bash et zsh : l'en-tête entre apostrophes, que le shell ne développe pas ; une invite, puis la
  // saisie masquée.
  await choose("bash ou zsh");
  let lines = commands();
  expect(lines[0]).toBe("printf 'Jeton : '; read -rs MEMENTUM_TOKEN; echo; export MEMENTUM_TOKEN");
  expect(lines[1]).toBe(
    `claude mcp add --transport http mementum '${ORIGIN}/mcp' --header 'Authorization: Bearer \${MEMENTUM_TOKEN}'`,
  );

  // PowerShell : entre apostrophes aussi (entre guillemets, il développerait sa propre variable,
  // vide) ; `-MaskInput` demande PowerShell 7.1, et la ligne de Windows PowerShell 5.1 est à côté.
  await choose("PowerShell");
  lines = commands();
  expect(lines[0]).toBe("$env:MEMENTUM_TOKEN = Read-Host -Prompt 'Jeton' -MaskInput");
  expect(lines[1]).toBe(
    "$env:MEMENTUM_TOKEN = [Net.NetworkCredential]::new('', (Read-Host -Prompt 'Jeton' -AsSecureString)).Password",
  );
  expect(lines[2]).toBe(
    `claude mcp add --transport http mementum '${ORIGIN}/mcp' --header 'Authorization: Bearer \${MEMENTUM_TOKEN}'`,
  );
  expect(host.textContent).toContain("Windows PowerShell 5.1");

  // cmd : entre guillemets (une apostrophe n'y groupe rien) ; pas de saisie masquée, et l'écran le dit,
  // avec la fenêtre à fermer une fois Claude Code branché.
  await choose("cmd");
  lines = commands();
  expect(lines[0]).toBe("set /p MEMENTUM_TOKEN=Jeton : ");
  expect(lines[1]).toBe(
    `claude mcp add --transport http mementum "${ORIGIN}/mcp" --header "Authorization: Bearer \${MEMENTUM_TOKEN}"`,
  );
  expect(host.textContent).toContain("ferme-la une fois Claude Code branché");

  // Dans tous les shells : où garder le jeton, une fois par shell ; le presse-papiers qui le garde ; la
  // portée de `claude mcp add`.
  expect(host.textContent).toContain("gestionnaire de mots de passe");
  expect(host.textContent).toContain("une fois par shell");
  expect(host.textContent).toContain("Win+V");
  expect(host.textContent).toContain("--scope user");
});

test("the_gestures_come_in_order", async () => {
  await rendered();
  for (const label of SHELL_LABELS) {
    await choose(label);
    // Dans l'ordre du document : la ligne qui attend le jeton, puis le jeton, puis la ligne qui branche.
    const steps = host.querySelector(".mm-token-steps")!;
    const order = [...steps.querySelectorAll<HTMLElement>("code.mm-token-command, input")].map((element) =>
      element.tagName === "INPUT" ? "jeton" : element.textContent!.startsWith("claude") ? "branche" : "lit",
    );
    expect(order.indexOf("lit"), label).toBe(0);
    expect(order.indexOf("jeton"), label).toBeGreaterThan(order.lastIndexOf("lit"));
    expect(order.indexOf("branche"), label).toBeGreaterThan(order.indexOf("jeton"));
    // Le premier geste : copier la ligne qui attend. Chaque bouton de copie a son nom.
    const first = host.querySelector<HTMLButtonElement>("[data-first-gesture]")!;
    expect(first.getAttribute("aria-label"), label).toBe("Copier la ligne 1");
    const names = [...steps.querySelectorAll("button")].map((b) => b.getAttribute("aria-label") ?? b.textContent);
    expect(new Set(names).size, label).toBe(names.length);
  }
});

test("an_address_a_shell_would_read_is_not_put_in_its_command", async () => {
  // Le caractère de citation du shell, `%`, `!`, un accent grave : la commande n'est pas écrite.
  const unsafe: [Shell, string][] = [
    ["bash", "http://a'b"],
    ["powershell", "http://a'b"],
    ["cmd", 'http://a"b'],
  ];
  for (const [shell, origin] of unsafe) expect(commandsFor(shell, origin), shell).toBeNull();
  for (const shell of ["bash", "powershell", "cmd"] as Shell[]) {
    for (const char of ["%", "!", "`", " "]) expect(commandsFor(shell, `http://a${char}b`), shell + char).toBeNull();
  }
  // Le caractère de citation d'un autre shell ne gêne pas celui-ci.
  expect(commandsFor("cmd", "http://a'b")).not.toBeNull();
  expect(commandsFor("bash", 'http://a"b')).not.toBeNull();

  // À l'écran : la raison, pas de commande, et le jeton toujours là, premier geste.
  await rendered("http://exemple%25");
  expect(commands()).toEqual([]);
  expect(host.textContent).toContain("la commande n'est pas montrée pour ce shell");
  const first = host.querySelector<HTMLButtonElement>("[data-first-gesture]")!;
  expect(first.textContent).toBe("Copier le jeton");
  expect([...host.querySelectorAll("input")].filter((input) => input.value === TOKEN)).toHaveLength(1);
});

test("the_default_shell_follows_the_system", async () => {
  const checked = () => labelOf(shells().find((input) => input.checked)!);
  platform = "Win32";
  await rendered();
  expect(checked()).toBe("PowerShell");
  await act(async () => root.unmount());
  root = createRoot(host);
  platform = "MacIntel";
  await rendered();
  expect(checked()).toBe("bash ou zsh");
});
