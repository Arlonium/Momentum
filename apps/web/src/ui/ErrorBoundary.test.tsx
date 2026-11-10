// La barrière d'erreur de l'application : une exception pendant le rendu montre un écran, au
// lieu de vider la page ; l'erreur va dans la console, jamais à l'écran.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { ErrorBoundary } from "./ErrorBoundary";

let host: HTMLDivElement;
let root: Root;
let logged: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // React écrit aussi dans la console les erreurs qu'une barrière attrape.
  logged = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  logged.mockRestore();
});

const render = (children: ReactNode) =>
  act(async () => root.render(<ErrorBoundary>{children}</ErrorBoundary>));

const SECRET = "détail interne : jeton d3adb33f, fichier App.tsx";

function Broken(): ReactNode {
  throw new Error(SECRET);
}

test("children_are_shown_when_nothing_fails", async () => {
  await render(<p>Tout va bien</p>);
  expect(host.textContent).toBe("Tout va bien");
  expect(logged).not.toHaveBeenCalled();
});

test("render_error_shows_a_screen_and_goes_to_the_console", async () => {
  await render(
    <>
      <p>Avant la panne</p>
      <Broken />
    </>,
  );
  // Un écran, dans le dessin des autres écrans d'erreur, à la place de la page vide.
  // (Une espace insécable avant « passé » : le mot ne reste pas seul sur sa ligne.)
  expect(host.querySelector("h1")?.textContent).toBe("Quelque chose s'est mal\u00a0passé");
  const reload = host.querySelector("button");
  expect(reload?.textContent).toBe("Recharger");
  // Rien de ce qui a échoué n'est montré : ni le message de l'erreur, ni sa pile.
  expect(host.textContent).not.toContain("Avant la panne");
  expect(host.textContent).not.toContain("détail interne");
  expect(host.textContent).not.toMatch(/Error|at \w+|\.tsx/);
  // L'erreur est dans la console, pour qui cherche la cause.
  const errors = logged.mock.calls.flat();
  expect(errors.some((value: unknown) => value instanceof Error && value.message === SECRET)).toBe(true);
});
