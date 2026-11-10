// La barrière d'erreur de l'application. Sans elle, une exception pendant le rendu démonte
// tout : l'écran devient blanc, sans un mot. Avec elle, un écran le dit, dans le dessin des
// autres écrans d'erreur, et propose de recharger. L'erreur est écrite dans la console, pour
// qui cherche la cause ; ni son message ni sa pile ne sont montrés.
//
// L'éditeur a sa propre barrière (`editor/LazyEditor.tsx`) : une page qui ne s'affiche pas
// laisse la barre latérale et le reste de l'application en place.
import { Component, type ErrorInfo, type ReactNode } from "react";
import { AuthScreen } from "../session/forms";

export class ErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error("Mementum : erreur pendant l'affichage.", error, info.componentStack);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <AuthScreen
        // Espace insécable : « passé » ne reste pas seul sur sa ligne.
        title={"Quelque chose s'est mal\u00a0passé"}
        lead="L'affichage s'est interrompu sur une erreur inattendue. Recharge la page pour reprendre."
      >
        <button
          type="button"
          className="mm-button mm-button-primary mm-auth-submit"
          onClick={() => location.reload()}
        >
          Recharger
        </button>
      </AuthScreen>
    );
  }
}
