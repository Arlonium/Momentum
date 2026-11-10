// L'application : barre latérale (sélecteur d'espace, sections et arbres des pages) et
// contenu. `/` mène au dernier espace ouvert (gardé dans le navigateur), sinon au premier du
// compte ; `/s/:spaceId` montre un espace, `/s/:spaceId/trash` sa corbeille,
// `/s/:spaceId/settings` ses réglages, `/p/:id` une page.
//
// L'espace affiché est celui que nomme l'adresse. Pour une page, c'est le sien : une page de
// l'arbre chargé le dit elle-même ; sinon (lien direct, page d'un autre espace) le serveur est
// interrogé (`getPage`), et répond `404` pour une page que la personne ne voit pas, comme
// pour une page qui n'existe pas. Un espace où le compte n'a pas, ou plus, de rôle ramène à
// son premier espace, sans rien en dire.
//
// Une page mise à la corbeille quitte l'arbre avec ses sous-pages ; ouverte, elle se lit
// encore, avec son bandeau (`PageView`). Une page déplacée ne bouge à l'écran qu'une fois le
// serveur d'accord : sur son refus, elle n'a pas quitté sa place, et son message s'affiche.
//
// La version de schéma du serveur est lue au chargement, puis relue avant chaque connexion de
// synchronisation (R35) : si elle diffère, tout passe en lecture seule, pour de bon.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import {
  ApiError,
  createPage,
  getInfo,
  getPage,
  getTree,
  isSessionLost,
  restorePage,
  trashPage,
  updatePage,
} from "./api";
import { ReadOnlyBanner } from "./editor/ReadOnlyBanner";
import { CLIENT_SCHEMA_VERSION } from "./editor/version";
import { PageView } from "./page/PageView";
import {
  navigate,
  pagePath,
  parseRoute,
  spacePath,
  useLinkInterception,
  usePathname,
} from "./router";
import { useSession } from "./session/session";
import { SpaceSettings } from "./settings/SpaceSettings";
import type { Drop } from "./sidebar/drag";
import { withoutSubtree } from "./sidebar/rows";
import type { CreateTarget } from "./sidebar/SectionList";
import { Sidebar } from "./sidebar/Sidebar";
import { CreateSpaceDialog } from "./sidebar/SpaceSwitcher";
import { preferredSpace, readLastSpace, writeLastSpace } from "./spaces";
import { TrashView } from "./trash/TrashView";

function messageOf(error: unknown): string {
  return error instanceof ApiError ? error.message : "Une erreur inattendue est survenue.";
}

/** Le message du serveur, en phrase : son initiale en capitale. */
function sentence(error: unknown): string {
  const message = messageOf(error);
  return message.charAt(0).toUpperCase() + message.slice(1);
}

/**
 * Le refus d'un déplacement, en phrase. Deux codes du serveur (`invalid_after`,
 * `invalid_parent`) parlent des champs de l'API, pas à la personne : on dit à leur place ce
 * qui s'est passé. Ils arrivent quand l'arbre a changé ailleurs entre-temps, ou dans
 * « Partagé », où deux pages voisines à l'écran peuvent être rangées sous des pages
 * différentes, que la personne ne voit pas.
 */
function moveRefusal(error: unknown): string {
  if (
    error instanceof ApiError &&
    (error.code === "invalid_after" || error.code === "invalid_parent")
  ) {
    return "Ces pages ne sont pas rangées ensemble dans l'espace, ou l'endroit visé a changé entre-temps.";
  }
  return sentence(error);
}

/** L'arbre chargé : les pages d'un espace que la personne voit, hors corbeille. */
type LoadedTree = { spaceId: string; nodes: PageNode[] };

/** Ce que le serveur dit d'une page ouverte qui n'est pas dans l'arbre chargé. */
type Lookup =
  | { id: string; state: "loading" }
  | { id: string; state: "found"; node: PageNode }
  /** Elle n'existe pas, ou la personne ne la voit pas : la même réponse. */
  | { id: string; state: "missing" }
  | { id: string; state: "failed"; message: string };

export function App() {
  useLinkInterception();
  const pathname = usePathname();
  const route = parseRoute(pathname);
  const { me, refresh } = useSession();
  const { spaces } = me;
  const [infoLoaded, setInfoLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tree, setTree] = useState<LoadedTree | null>(null);
  const treeRef = useRef(tree);
  treeRef.current = tree;
  const [lookup, setLookup] = useState<Lookup | null>(null);
  /** Le refus de la dernière action de la barre latérale ; il part à la navigation suivante. */
  const [notice, setNotice] = useState<string | null>(null);
  useEffect(() => setNotice(null), [pathname]);
  /** Le serveur n'a pas le schéma de ce client : plus aucune écriture (bandeau). */
  const [stale, setStale] = useState(false);
  const staleRef = useRef(false);
  /** Page tout juste créée : son titre prendra le focus, à sa première ouverture seulement. */
  const [created, setCreated] = useState<string | null>(null);
  const titleFocused = useCallback(() => setCreated(null), []);
  /** Le dernier espace affiché : il reste quand l'adresse n'en nomme aucun. */
  const [shown, setShown] = useState(() => preferredSpace(spaces, readLastSpace())?.id ?? null);
  /** Compte les changements faits ici à l'arbre : une lecture commencée avant est périmée. */
  const mutations = useRef(0);
  /** Demande une nouvelle lecture de l'arbre. */
  const [reloads, setReloads] = useState(0);
  /** La session s'est perdue pendant une lecture : à refaire quand elle revient. */
  const lost = useRef(false);
  const [retries, setRetries] = useState(0);
  useEffect(() => {
    if (!lost.current) return;
    lost.current = false;
    setRetries((n) => n + 1);
  }, [me]);

  const markStale = useCallback(() => {
    staleRef.current = true;
    setStale(true);
  }, []);

  /** Relit la version de schéma du serveur avant chaque connexion (R35). */
  const checkSchema = useCallback(async () => {
    const info = await getInfo();
    if (info.schemaVersion === CLIENT_SCHEMA_VERSION && !staleRef.current) return true;
    markStale();
    return false;
  }, [markStale]);

  useEffect(() => {
    let cancelled = false;
    getInfo()
      .then((info) => {
        if (cancelled) return;
        if (info.schemaVersion !== CLIENT_SCHEMA_VERSION) markStale();
        setInfoLoaded(true);
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoadError(messageOf(error));
      });
    return () => {
      cancelled = true;
    };
  }, [markStale]);

  // La page ouverte : celle de l'arbre chargé, sinon celle que le serveur a rendue.
  const activeId = route.name === "page" ? route.id : null;
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;
  const inTree = activeId === null ? undefined : tree?.nodes.find((n) => n.id === activeId);
  const looked = lookup !== null && lookup.id === activeId ? lookup : null;
  const active = inTree ?? (looked?.state === "found" ? looked.node : undefined);

  // L'espace affiché : celui que nomme l'adresse (pour une page, le sien), si le compte y a
  // un rôle ; sinon le dernier affiché, sinon le premier du compte.
  const named =
    route.name === "space" || route.name === "trash" || route.name === "spaceSettings"
      ? route.spaceId
      : (active?.spaceId ?? null);
  const space =
    spaces.find((s) => s.id === named) ?? spaces.find((s) => s.id === shown) ?? spaces[0] ?? null;
  const spaceId = space?.id ?? null;

  useEffect(() => {
    if (spaceId === null) return;
    setShown(spaceId);
    writeLastSpace(spaceId);
  }, [spaceId]);

  // L'adresse nomme un espace que le compte n'a pas : sa liste d'espaces est relue une fois
  // (il vient peut-être de le rejoindre ailleurs).
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

  // La page ouverte n'est pas dans l'arbre chargé (lien direct, page d'un autre espace, page
  // créée ailleurs depuis) : le serveur dit si la personne la voit, et dans quel espace.
  const outsideTree = activeId !== null && inTree === undefined;
  useEffect(() => {
    if (activeId === null || !outsideTree) return;
    let cancelled = false;
    // Déjà rendue par le serveur : elle reste à l'écran pendant qu'il est relu.
    setLookup((current) =>
      current !== null && current.id === activeId && current.state === "found"
        ? current
        : { id: activeId, state: "loading" },
    );
    getPage(activeId).then(
      (node) => {
        if (!cancelled) setLookup({ id: activeId, state: "found", node });
      },
      (error: unknown) => {
        if (cancelled) return;
        if (isSessionLost(error)) lost.current = true;
        else if (error instanceof ApiError && error.status === 404) {
          setLookup({ id: activeId, state: "missing" });
        } else setLookup({ id: activeId, state: "failed", message: messageOf(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [activeId, outsideTree, retries]);

  // L'arbre de l'espace affiché. Tant que le serveur n'a pas dit dans quel espace est la page
  // ouverte, on ne sait pas lequel charger.
  const resolving =
    activeId !== null && active === undefined && (looked === null || looked.state === "loading");
  const treeSpace = resolving ? null : spaceId;
  useEffect(() => {
    if (treeSpace === null) return;
    let cancelled = false;
    const load = async () => {
      for (;;) {
        const before = mutations.current;
        const fetched = await getTree(treeSpace);
        if (cancelled) return;
        // Une page créée ou renommée ici pendant la lecture : la réponse date d'avant.
        if (before !== mutations.current) continue;
        setTree({ spaceId: treeSpace, nodes: fetched.nodes });
        return;
      }
    };
    load().catch((error: unknown) => {
      if (cancelled) return;
      if (isSessionLost(error)) lost.current = true;
      // Plus de rôle dans cet espace : la liste des espaces du compte est relue, et l'adresse
      // mènera ailleurs.
      else if (error instanceof ApiError && error.status === 404) void refresh().catch(() => {});
      else setLoadError(messageOf(error));
    });
    return () => {
      cancelled = true;
    };
  }, [treeSpace, reloads, retries, refresh]);

  /** Les pages de l'espace affiché ; `null` : pas encore chargées. */
  const nodes = tree !== null && tree.spaceId === spaceId ? tree.nodes : null;

  // Une page que le serveur rend, hors corbeille, mais que l'arbre chargé de son espace n'a
  // pas (créée ailleurs depuis) : l'arbre est relu une fois.
  const [reloadedFor, setReloadedFor] = useState<string | null>(null);
  useEffect(() => setReloadedFor(null), [activeId]);
  useEffect(() => {
    if (looked?.state !== "found" || looked.node.trashedAt !== null || nodes === null) return;
    if (reloadedFor === looked.id || nodes.some((n) => n.id === looked.id)) return;
    setReloadedFor(looked.id);
    setReloads((n) => n + 1);
  }, [looked, nodes, reloadedFor]);

  const create = useCallback(
    async (target: CreateTarget) => {
      if (staleRef.current) return;
      setNotice(null);
      try {
        // Sans `afterId` : en fin des frères (R23). À la racine, il faut dire l'espace et la
        // section ; une sous-page est dans l'espace de sa page parente, et hérite de ses accès.
        const node = await createPage(
          "parentId" in target ? target : { spaceId, section: target.section },
        );
        mutations.current += 1;
        setTree((current) =>
          current !== null && current.spaceId === node.spaceId
            ? { ...current, nodes: [...current.nodes, node] }
            : current,
        );
        setCreated(node.id);
        navigate(pagePath(node.id));
      } catch (error) {
        setNotice(`La page n'a pas pu être créée. ${sentence(error)}`);
      }
    },
    [spaceId],
  );
  /** Stable : les lignes de l'arbre, mémoïsées, ne se rendent pas à chaque navigation (R43). */
  const createFromTree = useCallback((target: CreateTarget) => void create(target), [create]);

  const saved = useCallback((node: PageNode) => {
    mutations.current += 1;
    setTree(
      (current) =>
        current && { ...current, nodes: current.nodes.map((n) => (n.id === node.id ? node : n)) },
    );
    setLookup((current) =>
      current !== null && current.id === node.id && current.state === "found"
        ? { ...current, node }
        : current,
    );
  }, []);

  const trash = useCallback(async (id: string) => {
    setNotice(null);
    try {
      const trashed = await trashPage(id);
      mutations.current += 1;
      // La page ouverte part avec elle (elle-même, ou une de ses pages parentes) : elle reste
      // à l'écran, à la corbeille, le temps que le serveur le confirme.
      const before = treeRef.current?.nodes ?? [];
      const open = before.find((n) => n.id === activeIdRef.current);
      if (open !== undefined && !withoutSubtree(before, id).includes(open)) {
        setLookup({
          id: open.id,
          state: "found",
          node: open.id === id ? trashed : { ...open, trashedAt: trashed.trashedAt },
        });
      }
      setTree(
        (current) => current && { ...current, nodes: withoutSubtree(current.nodes, id) },
      );
    } catch (error) {
      // Session perdue : la connexion est demandée par-dessus, cela suffit.
      if (isSessionLost(error)) return;
      setNotice(`La page n'a pas pu être mise à la corbeille. ${sentence(error)}`);
    }
  }, []);
  /** Stable, comme `createFromTree`. */
  const trashFromTree = useCallback((id: string) => void trash(id), [trash]);

  const move = useCallback(async (id: string, drop: Drop) => {
    const node = treeRef.current?.nodes.find((n) => n.id === id);
    if (node === undefined) return;
    setNotice(null);
    // Parmi les mêmes sœurs, la page garde sa page parente, qu'on ne nomme pas : celle que le
    // serveur lui connaît n'est pas toujours visible d'ici (`parentId` vaut alors `null`).
    const sameParent = drop.parentId === node.parentId;
    try {
      const moved = await updatePage(id, sameParent ? { afterId: drop.afterId } : drop);
      mutations.current += 1;
      setTree(
        (current) =>
          current && { ...current, nodes: current.nodes.map((n) => (n.id === id ? moved : n)) },
      );
      // Sous une autre page, la page et ses sous-pages héritent d'autres accès : l'arbre est
      // relu.
      if (!sameParent) setReloads((n) => n + 1);
    } catch (error) {
      // Session perdue : la connexion est demandée par-dessus, cela suffit.
      if (isSessionLost(error)) return;
      // La page n'a pas bougé. Ce qui a fait refuser a pu changer ailleurs : la barre latérale
      // est relue, telle que le serveur la voit.
      setNotice(`La page n'a pas pu être déplacée. ${moveRefusal(error)}`);
      setReloads((n) => n + 1);
    }
  }, []);
  /** Stable, comme `createFromTree`. */
  const moveFromTree = useCallback((id: string, drop: Drop) => void move(id, drop), [move]);

  /** Une page est sortie de la corbeille : l'arbre de l'espace est relu. */
  const restored = useCallback(() => {
    mutations.current += 1;
    setReloads((n) => n + 1);
  }, []);

  const restore = useCallback(
    async (id: string) => {
      const node = await restorePage(id);
      setLookup((current) =>
        current !== null && current.id === id ? { id, state: "found", node } : current,
      );
      // L'arbre est relu ici : pas besoin de le relire pour cette page absente.
      setReloadedFor(id);
      restored();
    },
    [restored],
  );

  const settingsOf = route.name === "spaceSettings" ? route.spaceId : null;
  /** L'espace que montre l'adresse (son accueil ou sa corbeille), si le compte y a un rôle. */
  const atSpace = inSpace && unknown === null ? space : null;

  useEffect(() => {
    // Les réglages et la corbeille nomment l'onglet eux-mêmes.
    if (settingsOf !== null || route.name === "trash") return;
    const title = active?.title ?? atSpace?.name;
    document.title = title === undefined ? "Mementum" : `${title} · Mementum`;
  }, [active, atSpace, settingsOf, route.name]);

  /** Créer une page à la racine : pas pour un invité, ni en lecture seule. */
  const mayCreate = !stale && space !== null && space.role !== "guest";
  const [creatingSpace, setCreatingSpace] = useState(false);

  let content;
  if (settingsOf !== null) {
    // Sans attendre l'arbre des pages : les réglages ont leurs propres appels.
    content = <SpaceSettings key={settingsOf} spaceId={settingsOf} />;
  } else if (loadError) {
    content = (
      <Message title="Impossible de charger Mementum">
        {loadError} Recharge la page pour réessayer.
      </Message>
    );
  } else if (!infoLoaded) {
    content = <Message title="Chargement…" />;
  } else if (route.name === "home" && space === null) {
    content = (
      <Message title="Tu n'as aucun espace">
        Un espace range tes pages, et celles que tu partages. Crée le tien pour commencer.
        <span className="mm-message-actions">
          <button
            type="button"
            className="mm-button mm-button-primary"
            onClick={() => setCreatingSpace(true)}
          >
            Créer un espace
          </button>
        </span>
      </Message>
    );
  } else if (route.name === "home" || (inSpace && atSpace === null)) {
    // En route vers un espace du compte.
    content = <Message title="Chargement…" />;
  } else if (route.name === "trash" && atSpace !== null) {
    content = <TrashView key={atSpace.id} space={atSpace} onRestored={restored} />;
  } else if (route.name === "space") {
    content = (
      <Message title="Aucune page ouverte" banner={stale ? <ReadOnlyBanner /> : null}>
        Choisis une page dans la barre latérale{mayCreate ? ", ou crée-en une." : "."}
        {mayCreate && (
          <span className="mm-message-actions">
            <button
              type="button"
              className="mm-button mm-button-primary"
              onClick={() => void create({ section: "private" })}
            >
              Nouvelle page
            </button>
          </span>
        )}
      </Message>
    );
  } else if (active) {
    content = (
      <PageView
        key={active.id}
        node={active}
        readOnly={stale}
        checkSchema={checkSchema}
        autoFocusTitle={created === active.id}
        onTitleFocused={titleFocused}
        onSaved={saved}
        onRestore={restore}
      />
    );
  } else if (looked?.state === "failed") {
    content = (
      <Message title="Impossible d'ouvrir la page">
        {looked.message} Recharge la page pour réessayer.
      </Message>
    );
  } else if (activeId !== null && looked?.state !== "missing") {
    content = <Message title="Chargement…" />;
  } else {
    content = (
      <Message title="Page introuvable">
        Cette page n'existe pas, ou plus. Choisis-en une autre dans la barre latérale.
      </Message>
    );
  }

  return (
    <div className="mm-app">
      <Sidebar
        space={space}
        nodes={nodes}
        activeId={activeId}
        settingsOpen={settingsOf !== null && settingsOf === spaceId}
        trashOpen={route.name === "trash" && atSpace !== null}
        readOnly={stale}
        onCreate={createFromTree}
        onTrash={trashFromTree}
        onMove={moveFromTree}
        notice={notice}
      />
      <main className="mm-main">{content}</main>
      {creatingSpace && <CreateSpaceDialog onClose={() => setCreatingSpace(false)} />}
    </div>
  );
}

function Message({
  title,
  banner,
  children,
}: {
  title: string;
  banner?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <section className="mm-message">
      {banner}
      <h1>{title}</h1>
      {children && <p>{children}</p>}
    </section>
  );
}
