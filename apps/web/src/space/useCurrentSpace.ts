// L'espace affiché. C'est celui que nomme l'adresse : `/s/:spaceId` et ce qui suit, ou, pour
// une page (`/p/:id`), le sien. Quand l'adresse n'en nomme aucun (ou pas encore : le serveur
// n'a pas dit où est la page), le dernier espace affiché reste ; à défaut, le dernier ouvert
// dans ce navigateur (`spaces.ts`), sinon le premier du compte.
//
// `/` mène à cet espace, à la place de son adresse : ce n'est pas un lien vers un espace, et
// un dernier espace gardé qui n'est plus là retombe sur le premier sans rien dire. Une adresse
// qui nomme un espace où le compte n'a pas de rôle, elle, reste (Ruling R96) : la liste de ses
// espaces est relue une fois (il vient peut-être de le rejoindre ailleurs), puis l'espace est
// dit introuvable (`missing`), qu'il existe ou non.
import type { SpaceInfo } from "@mementum/protocol";
import { useEffect, useLayoutEffect, useState } from "react";
import { navigate, spacePath, type Route } from "../router";
import { preferredSpace, readLastSpace, writeLastSpace } from "../spaces";

export type CurrentSpace = {
  /** L'espace affiché ; `null` : le compte n'en a aucun. */
  space: SpaceInfo | null;
  /**
   * L'espace que montre l'adresse (son accueil ou sa corbeille), si le compte y a un rôle ;
   * `null` : l'adresse n'est pas celle d'un espace, ou le compte n'y a pas de rôle.
   */
  atSpace: SpaceInfo | null;
  /**
   * L'adresse nomme un espace (son accueil, sa corbeille, ses réglages) où le compte n'a pas
   * de rôle, liste de ses espaces relue.
   */
  missing: boolean;
};

type Inputs = {
  route: Route;
  /** L'espace de la page ouverte, quand on le connaît. */
  pageSpaceId: string | null;
  /** Les espaces du compte. */
  spaces: SpaceInfo[];
  /** Relit le compte et ses espaces. */
  refresh: () => Promise<void>;
};

/** L'espace dont l'adresse est celle (accueil, corbeille, réglages), sinon `null`. */
function addressed(route: Route): string | null {
  return route.name === "space" || route.name === "trash" || route.name === "spaceSettings"
    ? route.spaceId
    : null;
}

/**
 * L'espace à afficher : `named` (celui de l'adresse) si le compte y a un rôle, sinon `shown`
 * (le dernier affiché), sinon son premier espace.
 */
export function resolveSpace(
  spaces: SpaceInfo[],
  named: string | null,
  shown: string | null,
): SpaceInfo | null {
  return (
    spaces.find((s) => s.id === named) ?? spaces.find((s) => s.id === shown) ?? spaces[0] ?? null
  );
}

export function useCurrentSpace({ route, pageSpaceId, spaces, refresh }: Inputs): CurrentSpace {
  /** Le dernier espace affiché : il reste quand l'adresse n'en nomme aucun. */
  const [shown, setShown] = useState(() => preferredSpace(spaces, readLastSpace())?.id ?? null);
  // L'espace que nomme l'adresse : le sien, ou celui de la page ouverte.
  const named = addressed(route) ?? pageSpaceId;
  const space = resolveSpace(spaces, named, shown);
  const spaceId = space?.id ?? null;

  useEffect(() => {
    if (spaceId === null) return;
    setShown(spaceId);
    writeLastSpace(spaceId);
  }, [spaceId]);

  // L'adresse nomme un espace que le compte n'a pas : sa liste d'espaces est relue une fois.
  const unknown = named !== null && !spaces.some((s) => s.id === named) ? named : null;
  const [checked, setChecked] = useState<string | null>(null);
  useEffect(() => {
    if (unknown === null) {
      setChecked(null);
      return;
    }
    if (checked === unknown) return;
    let cancelled = false;
    void refresh()
      .catch(() => {})
      .then(() => {
        if (!cancelled) setChecked(unknown);
      });
    return () => {
      cancelled = true;
    };
  }, [unknown, checked, refresh]);

  // `/` : l'espace à ouvrir, à la place de cette adresse.
  const home = route.name === "home" ? spaceId : null;
  useLayoutEffect(() => {
    if (home !== null) navigate(spacePath(home), { replace: true });
  }, [home]);

  const inSpace = route.name === "space" || route.name === "trash";
  return {
    space,
    atSpace: inSpace && unknown === null ? space : null,
    // Toujours pas de rôle dans l'espace de l'adresse, liste relue. Rien ne dit s'il existe.
    missing: addressed(route) !== null && unknown !== null && checked === unknown,
  };
}
