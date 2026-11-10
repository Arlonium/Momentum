// L'espace affiché. C'est celui que nomme l'adresse : `/s/:spaceId` et ce qui suit, ou, pour
// une page (`/p/:id`), le sien. Quand l'adresse n'en nomme aucun (ou pas encore : le serveur
// n'a pas dit où est la page), le dernier espace affiché reste ; à défaut, le dernier ouvert
// dans ce navigateur (`spaces.ts`), sinon le premier du compte.
//
// `/` mène à cet espace, à la place de son adresse. Un espace où le compte n'a pas de rôle :
// sa liste d'espaces est relue une fois (il vient peut-être de le rejoindre ailleurs), puis
// l'adresse ramène à son premier espace, sans rien dire de l'autre.
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

/** L'espace que nomme l'adresse : le sien, ou celui de la page ouverte. */
function namedBy(route: Route, pageSpaceId: string | null): string | null {
  return route.name === "space" || route.name === "trash" || route.name === "spaceSettings"
    ? route.spaceId
    : pageSpaceId;
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
  const named = namedBy(route, pageSpaceId);
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

  // Toujours pas de rôle dans cet espace : l'accueil, donc le premier espace du compte, à la
  // place de cette adresse. Rien ne dit si l'espace existe.
  const inSpace = route.name === "space" || route.name === "trash";
  const gone = inSpace && unknown !== null && checked === unknown;
  // `/` : l'espace à ouvrir, à la place de cette adresse.
  const home = route.name === "home" ? spaceId : null;
  useLayoutEffect(() => {
    if (gone) navigate("/", { replace: true });
    else if (home !== null) navigate(spacePath(home), { replace: true });
  }, [gone, home]);

  return { space, atSpace: inSpace && unknown === null ? space : null };
}
