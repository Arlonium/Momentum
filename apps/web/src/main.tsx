// Point d'entrée. Inter est servie avec l'application (pas de police distante) ; les styles
// de l'éditeur arrivent avec son code (`editor/LazyEditor.tsx`). L'application ne se montre
// qu'avec une session (`SessionGate`) : sinon, la connexion ou l'inscription.
// Pas de `StrictMode` : il créerait deux fois l'éditeur collaboratif et sa connexion.
import "@blocknote/core/fonts/inter.css";
import "./theme.css";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { SessionGate } from "./session/SessionGate";

createRoot(document.getElementById("root")!).render(
  <SessionGate>
    <App />
  </SessionGate>,
);
