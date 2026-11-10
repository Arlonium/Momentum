// Point d'entrée. Inter est servie avec l'application (pas de police distante) ; les styles
// de l'éditeur arrivent avec son code (`editor/LazyEditor.tsx`). L'application ne se montre
// qu'avec une session (`SessionGate`) : sinon, la connexion ou l'inscription.
// Pas de `StrictMode` : il créerait deux fois l'éditeur collaboratif et sa connexion.
import "@blocknote/core/fonts/inter.css";
import "./theme.css";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { loadEditor } from "./editor/LazyEditor";
import { reloadWhenRestored } from "./session/bfcache";
import { SessionGate } from "./session/SessionGate";

// Rendu par le cache arrière-avant (Précédent après une déconnexion) : rechargé, pour que la
// session soit relue au lieu d'afficher l'application d'avant.
reloadWhenRestored(window, () => location.reload());

// Le code de l'éditeur arrive pendant que la session est lue et que l'arbre s'affiche. Un
// échec n'a pas d'effet ici : l'ouverture d'une page le redemande, et dit s'il manque encore.
loadEditor().catch(() => {});

createRoot(document.getElementById("root")!).render(
  <SessionGate>
    <App />
  </SessionGate>,
);
