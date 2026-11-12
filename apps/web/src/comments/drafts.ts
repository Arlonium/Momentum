// Les brouillons des commentaires d'une page ouverte : ce qu'on a tapé et pas encore envoyé, par
// cible (le commentaire de la page, celui d'un bloc, la réponse à un fil, la modification d'un
// message). Ils vivent là où le panneau est câblé (`useCommentsPanel`), hors des composants qui
// se démontent : fermer puis rouvrir le panneau, cliquer une pastille, relancer « Commenter » les
// retrouve. Un brouillon part quand son texte est envoyé, ou explicitement annulé. En mémoire
// seulement : rien dans le stockage du navigateur.

export type Drafts = {
  /** Le texte du brouillon ; `""` s'il n'y en a pas. */
  get(key: string): string;
  /** Garde le texte ; un texte vide retire le brouillon. */
  set(key: string, text: string): void;
  drop(key: string): void;
  has(key: string): boolean;
};

/** Les cibles d'un brouillon. */
export const DRAFT = {
  page: "page",
  block: (blockId: string) => `block:${blockId}`,
  reply: (threadId: string) => `reply:${threadId}`,
  edit: (commentId: string) => `edit:${commentId}`,
};

export function createDrafts(): Drafts {
  const texts = new Map<string, string>();
  return {
    get: (key) => texts.get(key) ?? "",
    set: (key, text) => {
      if (text === "") texts.delete(key);
      else texts.set(key, text);
    },
    drop: (key) => {
      texts.delete(key);
    },
    has: (key) => texts.has(key),
  };
}
