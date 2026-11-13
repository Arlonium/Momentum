-- Lot 3, partie B : les fichiers d'une page (Rulings R244, R245) et les favoris d'un compte
-- (R254).
--
-- Une migration pour la partie B, à elle (Ruling R243, qui amende R139) : `0003` est figée à la
-- clôture de la partie A, et une base où elle est appliquée ne s'ouvrirait plus si elle changeait.
-- Celle-ci peut changer jusqu'à la clôture de la partie B : aucun binaire de la partie B n'ouvre
-- d'ici là les données d'une instance que quelqu'un regarde.
--
-- Un contenu (`files`) est rangé sur le disque sous son SHA-256 : son chemin se calcule du hash,
-- d'où l'absence de `storage_key` (R244). Il est partagé par toutes les attaches du même contenu.
-- Une attache (`attachments`) dit qu'une page porte un fichier, sous un nom d'affichage ; c'est
-- elle que nomme l'adresse du fichier. Elle part avec sa page (`ON DELETE CASCADE`) ; le contenu,
-- lui, reste : seul le ménage le retire, quand plus aucune attache ne le nomme (R245). La clé
-- `attachments.file_hash` n'a pas de cascade : la base refuse de retirer un contenu encore
-- attaché.
--
-- Un favori (`favorites`) est une ligne `(compte, page)`, qui part avec sa page. Il ne change que
-- la barre latérale de son auteur : ni `rev` ni `lsn` ne bougent quand on le pose ou le retire
-- (R254).
--
-- Chaque clé que suit une suppression a un index qui commence par elle : `attachments.node_id` et
-- `favorites.node_id` (la suppression d'une page), `attachments.file_hash` (le ménage, qui cherche
-- les attaches d'un contenu avant de le retirer). Sans lui, SQLite relirait toute la table fille
-- pour chaque ligne supprimée (voir `0002`). `favorites.user_id` est en tête de sa clé primaire.

CREATE TABLE files (
    hash       BLOB PRIMARY KEY NOT NULL,   -- SHA-256, 32 octets
    size       INTEGER NOT NULL,
    mime       TEXT NOT NULL,               -- lu par le serveur dans les premiers octets (R246)
    created_at INTEGER NOT NULL
);

CREATE TABLE attachments (
    id         TEXT PRIMARY KEY NOT NULL,
    node_id    TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    file_hash  BLOB NOT NULL REFERENCES files(hash),
    name       TEXT NOT NULL,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
);
CREATE INDEX attachments_by_node ON attachments (node_id, created_at);
CREATE INDEX attachments_by_file ON attachments (file_hash);

CREATE TABLE favorites (
    user_id    TEXT NOT NULL REFERENCES users(id),
    node_id    TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, node_id)
);
CREATE INDEX favorites_by_node ON favorites (node_id);
