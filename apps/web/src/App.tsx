// L'application : barre latérale (arbre des pages) et page ouverte. `/` montre l'accueil,
// `/p/:id` une page. La version de schéma du serveur est lue une fois, au chargement.
import type { Info, PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { ApiError, createPage, getInfo, getTree } from "./api";
import { PageView } from "./page/PageView";
import { navigate, pagePath, parseRoute, useLinkInterception, usePathname } from "./router";
import { PageTree } from "./sidebar/PageTree";

function messageOf(error: unknown): string {
  return error instanceof ApiError ? error.message : "Une erreur inattendue est survenue.";
}

export function App() {
  useLinkInterception();
  const route = parseRoute(usePathname());
  const [info, setInfo] = useState<Info | null>(null);
  const [nodes, setNodes] = useState<PageNode[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** Page tout juste créée : son titre prendra le focus, à sa première ouverture seulement. */
  const [created, setCreated] = useState<string | null>(null);
  const titleFocused = useCallback(() => setCreated(null), []);

  useEffect(() => {
    let cancelled = false;
    Promise.all([getInfo(), getTree()])
      .then(([serverInfo, tree]) => {
        if (cancelled) return;
        setInfo(serverInfo);
        // Une page créée pendant le chargement n'est peut-être pas dans cette réponse.
        setNodes((current) => {
          const loaded = new Set(tree.nodes.map((n) => n.id));
          return [...tree.nodes, ...(current ?? []).filter((n) => !loaded.has(n.id))];
        });
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoadError(messageOf(error));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const create = useCallback(async (parentId: string | null) => {
    setNotice(null);
    try {
      // Sans `afterId` : en fin des frères (R23).
      const node = await createPage(parentId === null ? {} : { parentId });
      setNodes((current) => [...(current ?? []), node]);
      setCreated(node.id);
      navigate(pagePath(node.id));
    } catch (error) {
      setNotice(`La page n'a pas pu être créée. ${messageOf(error)}`);
    }
  }, []);

  const saved = useCallback((node: PageNode) => {
    setNodes((current) => current?.map((n) => (n.id === node.id ? node : n)) ?? null);
  }, []);

  const activeId = route.name === "page" ? route.id : null;
  const active = activeId === null ? undefined : nodes?.find((n) => n.id === activeId);

  useEffect(() => {
    document.title = active ? `${active.title} · Mementum` : "Mementum";
  }, [active]);

  let content;
  if (loadError) {
    content = (
      <Message title="Impossible de charger Mementum">
        {loadError} Recharge la page pour réessayer.
      </Message>
    );
  } else if (info === null || nodes === null) {
    content = <Message title="Chargement…" />;
  } else if (route.name === "home") {
    content = (
      <Message title="Aucune page ouverte">
        Choisis une page dans la barre latérale, ou crée-en une.
        <span className="mm-message-actions">
          <button type="button" className="mm-button mm-button-primary" onClick={() => void create(null)}>
            Nouvelle page
          </button>
        </span>
      </Message>
    );
  } else if (route.name === "page" && active) {
    content = (
      <PageView
        key={active.id}
        node={active}
        serverSchemaVersion={info.schemaVersion}
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
      <PageTree nodes={nodes} activeId={activeId} onCreate={(id) => void create(id)} notice={notice} />
      <main className="mm-main">{content}</main>
    </div>
  );
}

function Message({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <section className="mm-message">
      <h1>{title}</h1>
      {children && <p>{children}</p>}
    </section>
  );
}
