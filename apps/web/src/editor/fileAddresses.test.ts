// L'adresse d'un fichier de l'instance, une seule règle pour le client et le serveur (Ruling R754) :
// `filePath` (ce que l'éditeur montre, `files.ts`) et `file_address` du serveur (ce que la copie
// d'une page rattache et récrit, `crates/mementum-server/src/routes/duplicate.rs`) lisent la même
// table de cas, `spec/files/addresses.json`. Un cas qui change d'un côté rougit ce test ou celui du
// serveur, `file_address_follows_the_shared_cases`.
import { expect, test } from "vitest";
import table from "../../../../spec/files/addresses.json";
import { filePath } from "./files";

test("filePath_follows_the_cases_shared_with_the_server", () => {
  expect(table.cases.length, "la table est lue").toBeGreaterThan(30);
  for (const { why, address, names } of table.cases) {
    const found = filePath(address);
    // Le serveur lit un identifiant sans casse : la page et le fichier se comparent en minuscules.
    const read = found === null ? null : { page: found.pageId.toLowerCase(), file: found.fileId.toLowerCase() };
    expect(read, `${why} : ${JSON.stringify(address)}`).toEqual(names);
  }
});
