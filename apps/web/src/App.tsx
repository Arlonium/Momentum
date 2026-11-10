// L'application : barre latérale (sélecteur d'espace, sections et arbres des pages) et
// contenu. `/` mène au dernier espace ouvert (gardé dans le navigateur), sinon au premier du
// compte ; `/s/:spaceId` montre un espace, `/s/:spaceId/trash` sa corbeille,
// `/s/:spaceId/settings` ses réglages, `/p/:id` une page.
//
// Ce fichier assemble. La logique est dans ses hooks, un sujet chacun :
// - `useServerSchema` : la version de schéma du serveur (R35), et la lecture seule ;
// - `useSpaceTree`, `useTreeReader` : l'arbre de l'espace affiché ;
// - `useOpenPage` : la page ouverte, celle de l'arbre ou celle que le serveur rend ;
// - `useCurrentSpace` : l'espace affiché, d'après l'adresse et la page ouverte ;
// - `usePageActions` : créer, renommer, jeter, restaurer, déplacer, et ce que la page
//   ouverte devient quand ses droits changent ailleurs.
import { useCallback, useEffect, useState } from "react";
import { ReadOnlyBanner } from "./editor/ReadOnlyBanner";
import { useServerSchema } from "./editor/useServerSchema";
import { PageView } from "./page/PageView";
import { useOpenPage } from "./page/useOpenPage";
import { parseRoute, useLinkInterception, usePathname } from "./router";
import { useSession } from "./session/session";
import { useRetryAfterLogin } from "./session/useRetryAfterLogin";
import { SpaceSettings } from "./settings/SpaceSettings";
import type { Drop } from "./sidebar/drag";
import type { CreateTarget } from "./sidebar/SectionList";
import { Sidebar } from "./sidebar/Sidebar";
import { CreateSpaceDialog } from "./sidebar/SpaceSwitcher";
import { SpaceNotFound } from "./space/SpaceNotFound";
import { useCurrentSpace } from "./space/useCurrentSpace";
import { usePageActions } from "./space/usePageActions";
import { useSpaceTree, useTreeCatchUp, useTreeReader } from "./space/useSpaceTree";
import { TrashView } from "./trash/TrashView";
import { firstClick } from "./ui/firstClick";
import { Message, Retry } from "./ui/Message";

export function App() {
  useLinkInterception();
  const pathname = usePathname();
  const route = parseRoute(pathname);
  const { me, refresh } = useSession();
  const schema = useServerSchema();
  const { retries, sessionLost } = useRetryAfterLogin(me);
  const tree = useSpaceTree();

  // La page ouverte, puis l'espace affiché, qui est le sien ; puis l'arbre de cet espace.
  const activeId = route.name === "page" ? route.id : null;
  const inTree = activeId === null ? undefined : tree.loaded?.nodes.find((n) => n.id === activeId);
  const page = useOpenPage(activeId, inTree, { retries, onSessionLost: sessionLost });
  const active = page.node;
  const { space, atSpace, missing } = useCurrentSpace({
    route,
    pageSpaceId: active?.spaceId ?? null,
    spaces: me.spaces,
    refresh,
  });
  const spaceId = space?.id ?? null;
  // Le compte relu, sans rien dire d'un échec : quand il n'a plus de rôle dans l'espace lu
  // (l'adresse reste, l'écran dit l'espace introuvable, R96), et au retour sur l'onglet (R126).
  const rereadAccount = useCallback(() => void refresh().catch(() => {}), [refresh]);
  // Tant que le serveur n'a pas dit dans quel espace est la page ouverte, on ne sait pas quel
  // arbre lire. Une lecture qui échoue ne retire rien de l'écran : elle se dit en pied de
  // barre latérale.
  const treeError = useTreeReader(tree, page.resolving ? null : spaceId, {
    retries,
    onSessionLost: sessionLost,
    onGone: rereadAccount,
    onReturn: rereadAccount,
  });
  /** Les pages de l'espace affiché ; `null` : pas encore chargées. */
  const nodes = tree.loaded !== null && tree.loaded.spaceId === spaceId ? tree.loaded.nodes : null;
  const found = page.looked?.state === "found" ? page.looked.node : null;
  const caughtUp = useTreeCatchUp(tree, nodes, activeId, found);

  const actions = usePageActions({
    tree,
    page,
    spaceId,
    activeId,
    pathname,
    isStale: schema.isStale,
    caughtUp,
  });
  const { create, trash, move } = actions;
  // Stables : les lignes de l'arbre, mémoïsées, ne se rendent pas à chaque navigation (R43).
  const createFromTree = useCallback((target: CreateTarget) => void create(target), [create]);
  const trashFromTree = useCallback((id: string) => void trash(id), [trash]);
  const moveFromTree = useCallback((id: string, drop: Drop) => void move(id, drop), [move]);

  const { stale } = schema;
  const settingsOf = route.name === "spaceSettings" ? route.spaceId : null;
  const inSpace = route.name === "space" || route.name === "trash";

  useEffect(() => {
    // Les réglages, la corbeille et l'espace introuvable nomment l'onglet eux-mêmes.
    if (settingsOf !== null || route.name === "trash" || missing) return;
    const title = active?.title ?? atSpace?.name;
    document.title = title === undefined ? "Mementum" : `${title} · Mementum`;
  }, [active, atSpace, settingsOf, route.name, missing]);

  /** Créer une page à la racine : pas pour un invité, ni en lecture seule. */
  const mayCreate = !stale && space !== null && space.role !== "guest";
  const [creatingSpace, setCreatingSpace] = useState(false);
  const { looked } = page;

  let content;
  if (missing) {
    // Un espace où le compte n'a pas de rôle : le même écran pour son accueil, sa corbeille
    // et ses réglages (R96).
    content = <SpaceNotFound first={me.spaces[0] ?? null} />;
  } else if (settingsOf !== null) {
    // Sans attendre l'arbre des pages : les réglages ont leurs propres appels. Un espace que
    // le compte n'a pas (encore) : on attend de savoir s'il vient de le rejoindre.
    content = me.spaces.some((s) => s.id === settingsOf) ? (
      <SpaceSettings key={settingsOf} spaceId={settingsOf} />
    ) : (
      <Message title="Chargement…" />
    );
  } else if (schema.error) {
    // Au premier chargement seulement : sans la version du serveur, rien ne peut s'ouvrir.
    content = (
      <Message title="Impossible de charger Mementum">
        {schema.error}
        <Retry onRetry={schema.retry} />
      </Message>
    );
  } else if (!schema.loaded) {
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
    content = (
      <TrashView
        key={atSpace.id}
        space={atSpace}
        version={actions.trashChanges}
        readOnly={stale}
        onChanged={actions.trashChanged}
      />
    );
  } else if (route.name === "space" && nodes === null && treeError !== null) {
    // L'arbre n'a jamais pu être lu : la barre latérale est vide, il n'y a rien à y choisir.
    content = (
      <Message title="La liste des pages n'a pas pu être lue">
        {treeError}
        <Retry onRetry={tree.reload} />
      </Message>
    );
  } else if (route.name === "space") {
    content = (
      <Message title="Aucune page ouverte" banner={stale ? <ReadOnlyBanner /> : null}>
        Choisis une page dans la barre latérale{mayCreate ? ", ou crée-en une." : "."}
        {mayCreate && (
          <span className="mm-message-actions">
            <button
              type="button"
              className="mm-button mm-button-primary"
              onClick={firstClick(() => void create({ section: "private" }))}
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
        checkSchema={schema.check}
        autoFocusTitle={actions.created === active.id}
        onTitleFocused={actions.titleFocused}
        onSaved={actions.saved}
        onRestore={actions.restore}
        onReread={actions.reread}
        onLost={actions.lost}
        onShared={actions.shared}
        onShareRefused={actions.shareRefused}
      />
    );
  } else if (looked?.state === "gone") {
    // Perdue pendant qu'elle était ouverte : le serveur répond comme pour une page qui
    // n'existe pas, qu'elle ait été supprimée ou que l'accès ait été retiré.
    content = (
      <Message title="Tu n'as plus accès à cette page">
        Elle a été supprimée, ou son accès t'a été retiré.
        {space !== null && " Choisis-en une autre dans la barre latérale."}
      </Message>
    );
  } else if (looked?.state === "failed") {
    content = (
      <Message title="Impossible d'ouvrir la page">
        {looked.message}
        <Retry onRetry={page.ask} />
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
        notice={actions.notice}
        readError={treeError}
        onRetry={tree.reload}
      />
      <main className="mm-main">{content}</main>
      {creatingSpace && <CreateSpaceDialog onClose={() => setCreatingSpace(false)} />}
    </div>
  );
}
