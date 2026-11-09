// L'éditeur, chargé à la demande. BlockNote, ProseMirror, TipTap, Mantine et KaTeX font
// l'essentiel du code de l'application : dans un chunk à part, ils ne retardent ni la
// barre latérale ni l'arbre des pages. L'application lance ce chargement dès son premier
// affichage (`App.tsx`), pour qu'il soit fait quand on ouvre une page.
import { Component, lazy, Suspense, type ComponentProps, type ReactNode } from "react";
import { WarningIcon } from "../icons";

/** Charge le module de l'éditeur (le navigateur ne le charge qu'une fois). */
export function loadEditor(): Promise<typeof import("./Editor")> {
  return import("./Editor");
}

const Editor = lazy(async () => ({ default: (await loadEditor()).Editor }));

/** `Editor`, affiché quand son code est arrivé. */
export function LazyEditor(props: ComponentProps<typeof Editor>) {
  return (
    <EditorBoundary>
      <Suspense fallback={null}>
        <Editor {...props} />
      </Suspense>
    </EditorBoundary>
  );
}

/**
 * Code de l'éditeur introuvable (réseau coupé, application mise à jour depuis) ou éditeur
 * en erreur : un message dans la page, au lieu d'une application vide.
 */
class EditorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="mm-banner" role="alert">
        <WarningIcon />
        <p>L'éditeur n'a pas pu s'afficher. Recharge la page pour réessayer.</p>
        <button type="button" className="mm-button" onClick={() => location.reload()}>
          Recharger
        </button>
      </div>
    );
  }
}
