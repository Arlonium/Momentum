// Les blocs de fichier d'une page qui nomment une adresse donnée (Task C9). Dans un module à lui : un
// parcours de toute la page ne se fait jamais à la frappe, et un test compte ses appels.
import type { MementumEditor } from "./debug";
import { FILE_BLOCKS } from "./files";

/**
 * Les blocs de fichier (`image`, `video`, `audio`, `file`) de la page dont l'adresse est, à la lettre,
 * l'une de `urls` : pour chaque id de bloc, son adresse. Un parcours des nœuds du document, sans
 * convertir les blocs (une page de 10 000 blocs se parcourt en quelques millisecondes).
 */
export function blocksNaming(editor: MementumEditor, urls: ReadonlySet<string>): Map<string, string> {
  const found = new Map<string, string>();
  editor.transact((tr) => {
    tr.doc.descendants((node, _pos, parent) => {
      if (!FILE_BLOCKS.has(node.type.name)) return true;
      const url: unknown = node.attrs.url;
      const id: unknown = parent?.attrs.id;
      if (typeof url === "string" && typeof id === "string" && urls.has(url)) found.set(id, url);
      return false;
    });
  });
  return found;
}
