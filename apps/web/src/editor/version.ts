// Version du schéma de blocs, à part du schéma BlockNote (`schema.tsx`) : l'application la
// lit au chargement sans attendre le code de l'éditeur, chargé à la demande
// (`LazyEditor.tsx`).

/**
 * Version du schéma de blocs de ce client, celle de `blocks.json`. Le serveur et la
 * WebSocket la comparent à la leur : un client d'une autre version passe en lecture seule.
 * À changer avec le registre, quand le schéma change.
 */
export const CLIENT_SCHEMA_VERSION = 1;
