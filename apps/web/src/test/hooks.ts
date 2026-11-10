// Outils des tests unitaires de hooks (Vitest, jsdom), sans dépendance de plus : un hook est
// rendu par un composant minimal, dans une racine React, sous `act`.
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";

declare global {
  // Dit à React que les mises à jour d'état des tests passent par `act`.
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

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
  const render = (value: P) => act(async () => root.render(createElement(Probe, { value })));
  await render(props);
  return {
    get current() {
      return current;
    },
    rerender: render,
    unmount: () => act(async () => root.unmount()),
  };
}

/** Exécute `work` (un changement d'état, une promesse qu'on tient), puis laisse React suivre. */
export async function settle(work: () => unknown = () => {}): Promise<void> {
  await act(async () => {
    await work();
  });
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
