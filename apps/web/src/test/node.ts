// Les modules de Node dont se servent quelques tests unitaires (lire ou écrire des octets du
// dépôt, charger un fichier d'une dépendance). Vitest tourne sous Node, mais le code de `src` est
// typé sans les types de Node (`tsconfig.json`) : ils sont chargés à l'exécution, et typés ici
// pour ce qui sert.

export type Fs = {
  readFile(path: string): Promise<Uint8Array>;
  readFile(path: string, encoding: "utf8"): Promise<string>;
  writeFile(path: string, data: Uint8Array | string): Promise<void>;
  mkdir(path: string, options: { recursive: true }): Promise<unknown>;
  readdir(path: string, options: { recursive: true }): Promise<string[]>;
};
export type Path = {
  dirname(path: string): string;
  join(...parts: string[]): string;
  resolve(...parts: string[]): string;
};
export type Url = {
  fileURLToPath(url: string): string;
  pathToFileURL(path: string): { href: string };
};
export type Module = {
  createRequire(from: string): { (name: string): unknown; resolve(name: string): string };
};
export type Crypto = {
  createHash(algorithm: "sha256"): { update(data: string): { digest(encoding: "hex"): string } };
};

/** Un module de Node (`node:fs/promises`, `node:path`…), chargé à l'exécution. */
export async function nodeModule<T>(name: string): Promise<T> {
  return (await import(/* @vite-ignore */ name)) as T;
}
