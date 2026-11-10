// Avant tous les tests, une fois les serveurs lancés : le compte par défaut. C'est le premier
// compte de l'instance (admin d'instance, avec son espace), et sa session
// (`defaultStorageState()`) est celle de tous les contextes des tests (`use.storageState`
// des deux configurations). `defaultAccount()` (`helpers.ts`) le relit.
import type { FullConfig } from "@playwright/test";
import { writeFileSync } from "node:fs";
import { createAccount, defaultAccountFile, PASSWORD } from "./helpers";
import { defaultStorageState } from "./servers";

export default async function globalSetup(config: FullConfig): Promise<void> {
  const baseURL = config.projects[0].use.baseURL;
  if (!baseURL) throw new Error("baseURL manquante dans la configuration de Playwright");
  const account = await createAccount(
    { email: "compte-par-defaut@exemple.fr", name: "Compte des tests", password: PASSWORD },
    defaultStorageState(),
    baseURL,
  );
  writeFileSync(defaultAccountFile(), JSON.stringify(account));
}
