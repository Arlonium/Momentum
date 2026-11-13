// Outils des tests unitaires de hooks (Vitest, jsdom), sans dépendance de plus : un hook est
// rendu par un composant minimal, dans une racine React, sous `act`.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach } from "vitest";

declare global {
  // Dit à React que les mises à jour d'état des tests passent par `act`.
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

/** Les racines des hooks rendus par le test en cours : démontées à sa fin. */
const roots = new Set<Root>();
afterEach(async () => {
  // Un hook resté monté écouterait encore la fenêtre pendant les tests suivants.
  const mounted = [...roots];
  roots.clear();
  await act(async () => {
    for (const root of mounted) root.unmount();
  });
});

export type RenderedHook<P, R> = {
  /** Ce que le hook a rendu à son dernier rendu. */
  readonly current: R;
  /** Rend de nouveau le hook, avec d'autres paramètres. */
  rerender(props: P): Promise<void>;
  unmount(): Promise<void>;
};

/**
 * Rend `hook(props)`, et attend ses effets et les promesses déjà tenues. Le type des
 * paramètres est celui du hook, pas celui des premiers qu'on lui donne.
 */
export async function renderHook<P, R>(
  hook: (props: P) => R,
  props: NoInfer<P>,
): Promise<RenderedHook<P, R>> {
  let current!: R;
  function Probe({ value }: { value: P }) {
    current = hook(value);
    return null;
  }
  const root = createRoot(document.createElement("div"));
  roots.add(root);
  const render = (value: P) => act(async () => root.render(createElement(Probe, { value })));
  await render(props);
  return {
    get current() {
      return current;
    },
    rerender: render,
    unmount: async () => {
      roots.delete(root);
      await act(async () => root.unmount());
    },
  };
}

/** Exécute `work` (un changement d'état, une promesse qu'on tient), puis laisse React suivre. */
export async function settle(work: () => unknown = () => {}): Promise<void> {
  await act(async () => {
    await work();
  });
}

/**
 * Attend que `check` soit vrai, en laissant React et les promesses suivre (toutes les 10 ms, en
 * temps réel : pas sous des minuteurs simulés). `host` : l'élément du test qui porte ce qu'il rend.
 * Dès qu'il a quitté le document, l'attente s'arrête, avant de relire `check` : un test coupé par le
 * délai de Vitest (son `afterEach` a retiré l'hôte) ne peut pas être satisfait par le test suivant,
 * qui réaffecte les variables du module, ni rendre ensuite dans sa racine. `bound` (ms, mesurée par
 * `performance.now`, que les tests qui figent `Date` laissent courir) reste sous le délai d'un test
 * de Vitest (5 s), pour que l'échec dise ce qu'on attendait.
 */
export async function until(
  host: Element,
  check: () => boolean,
  what: string,
  bound = 4_000,
): Promise<void> {
  const end = performance.now() + bound;
  for (;;) {
    if (!host.isConnected) throw new Error(`test fini : ${what}`);
    if (check()) return;
    if (performance.now() > end) throw new Error(`toujours pas : ${what}`);
    await act(async () => {
      await new Promise((done) => setTimeout(done, 10));
    });
  }
}

/** Une promesse que le test tient ou rompt quand il veut : une réponse du serveur. */
export type Deferred<T> = {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: unknown): void;
};

export function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // Une promesse rompue avant d'être attendue n'est pas une erreur du test.
  promise.catch(() => {});
  return { promise, resolve, reject };
}
