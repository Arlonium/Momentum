// L'application : barre latérale (arbre des pages) et page ouverte. `/` montre l'accueil,
// `/p/:id` une page, `/s/:spaceId/settings` les réglages d'un espace. La version de schéma
// du serveur est lue au chargement, puis relue avant chaque connexion de synchronisation
// (R35) : si elle diffère, tout passe en lecture seule, pour de bon.
//
// L'espace est le premier du compte (`me.spaces`) : son arbre, et les pages créées à sa
// racine, privées (le défaut du serveur). Le sélecteur d'espace et les sections viennent
// ensuite.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ApiError, createPage, getInfo, getTree } from "./api";
import { ReadOnlyBanner } from "./editor/ReadOnlyBanner";
import { CLIENT_SCHEMA_VERSION } from "./editor/version";
import { PageView } from "./page/PageView";
import { navigate, pagePath, parseRoute, useLinkInterception, usePathname } from "./router";
import { useSession } from "./session/session";
import { SpaceSettings } from "./settings/SpaceSettings";
import { PageTree } from "./sidebar/PageTree";

function messageOf(error: unknown): string {
  return error instanceof ApiError ? error.message : "Une erreur inattendue est survenue.";
}

/** Les pages du serveur, plus celles d'ici qu'une réponse plus ancienne n'avait pas. */
function merge(server: PageNode[], current: PageNode[] | null): PageNode[] {
  const known = new Set(server.map((n) => n.id));
  return [...server, ...(current ?? []).filter((n) => !known.has(n.id))];
}

export function App() {
  useLinkInterception();
  const route = parseRoute(usePathname());
  /** L'espace affiché ; `null` : le compte n'en a aucun. */
  const spaceId = useSession().me.spaces[0]?.id ?? null;
  const [loaded, setLoaded] = useState(false);
  const [nodes, setNodes] = useState<PageNode[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** Le serveur n'a pas le schéma de ce client : plus aucune écriture (bandeau). */
  const [stale, setStale] = useState(false);
  const staleRef = useRef(false);
  /** Page tout juste créée : son titre prendra le focus, à sa première ouverture seulement. */
  const [created, setCreated] = useState<string | null>(null);
  const titleFocused = useCallback(() => setCreated(null), []);
  /** Page absente de l'arbre pour laquelle l'arbre a déjà été relu une fois. */
  const [reloadedFor, setReloadedFor] = useState<string | null>(null);
  const [reloading, setReloading] = useState(false);

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
    Promise.all([getInfo(), spaceId === null ? null : getTree(spaceId)])
      .then(([info, tree]) => {
        if (cancelled) return;
        if (info.schemaVersion !== CLIENT_SCHEMA_VERSION) markStale();
        // Une page créée pendant le chargement n'est peut-être pas dans cette réponse.
        setNodes((current) => merge(tree?.nodes ?? [], current));
        setLoaded(true);
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoadError(messageOf(error));
      });
    return () => {
      cancelled = true;
    };
  }, [markStale, spaceId]);

  const create = useCallback(
    async (parentId: string | null) => {
      if (staleRef.current || spaceId === null) return;
      setNotice(null);
      try {
        // Sans `afterId` : en fin des frères (R23). À la racine, il faut dire l'espace ; une
        // sous-page est dans celui de sa page parente.
        const node = await createPage(parentId === null ? { spaceId } : { parentId });
        setNodes((current) => [...(current ?? []), node]);
        setCreated(node.id);
        navigate(pagePath(node.id));
      } catch (error) {
        setNotice(`La page n'a pas pu être créée. ${messageOf(error)}`);
      }
    },
    [spaceId],
  );
  /** Stable : les lignes de l'arbre, mémoïsées, ne se rendent pas à chaque navigation (R43). */
  const createFromTree = useCallback((parentId: string | null) => void create(parentId), [create]);

  const saved = useCallback((node: PageNode) => {
    setNodes((current) => current?.map((n) => (n.id === node.id ? node : n)) ?? null);
  }, []);

  const activeId = route.name === "page" ? route.id : null;
  const active = activeId === null ? undefined : nodes?.find((n) => n.id === activeId);
  // Page inconnue de l'arbre chargé (créée ailleurs depuis ?) : l'arbre est relu une fois.
  const missing = loaded && activeId !== null && !active ? activeId : null;

  useEffect(() => {
    if (missing === null || reloadedFor === missing) return;
    setReloadedFor(missing);
    if (spaceId === null) return;
    setReloading(true);
    getTree(spaceId)
      .then((tree) => setNodes((current) => merge(tree.nodes, current)))
      .catch(() => {
        // le message « Page introuvable » suffit
      })
      .finally(() => setReloading(false));
  }, [missing, reloadedFor, spaceId]);

  const settingsOf = route.name === "spaceSettings" ? route.spaceId : null;

  useEffect(() => {
    // Les réglages nomment l'onglet eux-mêmes.
    if (settingsOf !== null) return;
    document.title = active ? `${active.title} · Mementum` : "Mementum";
  }, [active, settingsOf]);

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
  } else if (!loaded || (missing !== null && (reloading || reloadedFor !== missing))) {
    content = <Message title="Chargement…" />;
  } else if (route.name === "home") {
    content = (
      <Message title="Aucune page ouverte" banner={stale ? <ReadOnlyBanner /> : null}>
        Choisis une page dans la barre latérale{stale ? "." : ", ou crée-en une."}
        {!stale && (
          <span className="mm-message-actions">
            <button
              type="button"
              className="mm-button mm-button-primary"
              onClick={() => void create(null)}
            >
              Nouvelle page
            </button>
          </span>
        )}
      </Message>
    );
  } else if (route.name === "page" && active) {
    content = (
      <PageView
        key={active.id}
        node={active}
        readOnly={stale}
        checkSchema={checkSchema}
        autoFocusTitle={created === active.id}
        onTitleFocused={titleFocused}
        onSaved={saved}
      />
    );
  } else {
    content = (
      <Message title="Page introuvable">
        Cette page n'existe pas, ou plus. Choisis-en une autre dans la barre latérale.
      </Message>
    );
  }

  return (
    <div className="mm-app">
      <PageTree
        nodes={nodes}
        activeId={activeId}
        settingsOf={settingsOf}
        readOnly={stale}
        onCreate={createFromTree}
        notice={notice}
      />
      <main className="mm-main">{content}</main>
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
