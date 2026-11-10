-- Lot 2 : des espaces à la place des organisations et des coffres ; comptes, sessions, membres,
-- accès, invitations, corbeille.
--
-- Données du lot 1 : chaque coffre devient un espace de même id, « Mon espace », sans membre
-- (le premier compte créé l'adopte) ; ses nœuds gardent leur id, leur place et leur document ;
-- chaque nœud de racine reçoit l'accès `(members, full)`.
--
-- sqlx lance ce fichier dans une transaction, où `PRAGMA foreign_keys` ne se change pas : les
-- clés étrangères sont vérifiées tout du long. D'où l'ordre :
--   1. créer les tables neuves, et `nodes_new`, `doc_updates_new`, `doc_snapshots_new` (SQLite
--      ne retire pas une colonne qui porte une clé étrangère : on recrée) ;
--   2. copier, chaque table en une seule instruction : une clé étrangère immédiate n'est
--      vérifiée qu'à la fin de l'instruction, un enfant peut donc être lu avant son parent ;
--   3. supprimer les tables filles (`doc_updates`, `doc_snapshots`) avant `nodes`, puis
--      `vaults`, puis `orgs` : jamais une table mère qui a encore des lignes filles ;
--   4. renommer les `_new` : SQLite récrit alors les `REFERENCES nodes_new` en `REFERENCES nodes`,
--      y compris dans `acl` et `invitations` ;
--   5. recréer les index.
--
-- Quand une ligne d'une table mère est supprimée, SQLite cherche ses lignes filles par leur clé
-- étrangère ; sans index qui commence par cette clé, il relit toute la table fille, pour chaque
-- ligne supprimée. D'où deux index de plus que celui du lot 1, sur les clés que suit la
-- suppression d'un nœud (`nodes.parent_id`, `invitations.node_id`), et un index de passage avant
-- `DROP TABLE nodes`.

-- 1. Tables neuves.

CREATE TABLE users (
    id             TEXT PRIMARY KEY NOT NULL,
    -- Rognée et en minuscules.
    email          TEXT UNIQUE NOT NULL,
    name           TEXT NOT NULL,
    -- `NULL` : un compte sans mot de passe (connexion par OIDC, prévue plus tard).
    password_hash  TEXT,
    instance_admin INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL
);

CREATE TABLE sessions (
    -- SHA-256 du jeton : le jeton lui-même n'est jamais stocké.
    token_hash BLOB PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE spaces (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL,
    -- Incrémentée par toute écriture qui touche à la structure ou aux droits de l'espace.
    rev        INTEGER NOT NULL DEFAULT 0,
    created_by TEXT REFERENCES users(id),
    created_at INTEGER NOT NULL
);

CREATE TABLE members (
    space_id   TEXT NOT NULL REFERENCES spaces(id),
    user_id    TEXT NOT NULL REFERENCES users(id),
    role       TEXT NOT NULL CHECK (role IN ('owner','admin','member','guest')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (space_id, user_id)
);

CREATE TABLE nodes_new (
    id         TEXT PRIMARY KEY NOT NULL,
    space_id   TEXT NOT NULL REFERENCES spaces(id),
    parent_id  TEXT REFERENCES nodes_new(id),
    kind       TEXT NOT NULL CHECK (kind IN ('folder','page')),
    title      TEXT NOT NULL,
    position   TEXT NOT NULL,
    created_by TEXT REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_by TEXT REFERENCES users(id),
    updated_at INTEGER NOT NULL,
    trashed_at INTEGER,
    trashed_by TEXT REFERENCES users(id)
);

CREATE TABLE acl (
    node_id TEXT NOT NULL REFERENCES nodes_new(id) ON DELETE CASCADE,
    -- `user:<uuid>` ou `members` (forme texte de `mementum-acl`).
    subject TEXT NOT NULL,
    level   TEXT NOT NULL CHECK (level IN ('read','comment','edit','full')),
    PRIMARY KEY (node_id, subject)
);

CREATE TABLE invitations (
    id          TEXT PRIMARY KEY NOT NULL,
    space_id    TEXT NOT NULL REFERENCES spaces(id),
    -- `NULL` : invitation dans l'espace (`grant_role`) ; sinon sur cette page (`grant_level`).
    node_id     TEXT REFERENCES nodes_new(id) ON DELETE CASCADE,
    email       TEXT NOT NULL,
    grant_role  TEXT CHECK (grant_role IS NULL OR grant_role IN ('owner','admin','member','guest')),
    grant_level TEXT CHECK (grant_level IS NULL OR grant_level IN ('read','comment','edit','full')),
    -- SHA-256 du jeton : le jeton lui-même n'est jamais stocké.
    token_hash  BLOB UNIQUE NOT NULL,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    accepted_at INTEGER,
    accepted_by TEXT REFERENCES users(id),
    CHECK ((node_id IS NULL) = (grant_role IS NOT NULL)),
    CHECK ((node_id IS NULL) = (grant_level IS NULL))
);

CREATE TABLE doc_updates_new (
    node_id     TEXT NOT NULL REFERENCES nodes_new(id),
    seq         INTEGER NOT NULL,
    update_blob BLOB NOT NULL,
    created_at  INTEGER NOT NULL,
    -- Le compte qui a écrit ; `NULL` : inconnu (lot 1), ou le serveur.
    actor_id    TEXT REFERENCES users(id),
    PRIMARY KEY (node_id, seq)
);

CREATE TABLE doc_snapshots_new (
    node_id    TEXT PRIMARY KEY NOT NULL REFERENCES nodes_new(id),
    seq        INTEGER NOT NULL,
    state_blob BLOB NOT NULL,
    created_at INTEGER NOT NULL
);

-- 2. Copie.

INSERT INTO spaces (id, name, rev, created_by, created_at)
SELECT id, 'Mon espace', 0, NULL, created_at FROM vaults;

INSERT INTO nodes_new (id, space_id, parent_id, kind, title, position,
                       created_by, created_at, updated_by, updated_at, trashed_at, trashed_by)
SELECT id, vault_id, parent_id, kind, title, position,
       NULL, created_at, NULL, updated_at, trashed_at, NULL
FROM nodes;

INSERT INTO acl (node_id, subject, level)
SELECT id, 'members', 'full' FROM nodes_new WHERE parent_id IS NULL;

INSERT INTO doc_updates_new (node_id, seq, update_blob, created_at, actor_id)
SELECT node_id, seq, update_blob, created_at, NULL FROM doc_updates;

INSERT INTO doc_snapshots_new (node_id, seq, state_blob, created_at)
SELECT node_id, seq, state_blob, created_at FROM doc_snapshots;

-- 3. Suppression, les tables filles d'abord.

DROP TABLE doc_updates;
DROP TABLE doc_snapshots;
-- `nodes` se vise elle-même (`parent_id`) : `DROP TABLE` en supprime donc les lignes une à une,
-- en cherchant les enfants de chacune. L'index part avec la table.
CREATE INDEX nodes_lot1_parent ON nodes (parent_id);
DROP TABLE nodes;
DROP TABLE vaults;
DROP TABLE orgs;

-- 4. Renommage : `nodes` d'abord, pour que les tables qui la visent suivent.

ALTER TABLE nodes_new RENAME TO nodes;
ALTER TABLE doc_updates_new RENAME TO doc_updates;
ALTER TABLE doc_snapshots_new RENAME TO doc_snapshots;

-- 5. Index : l'arbre d'un espace dans l'ordre, puis les clés que suit la suppression d'un nœud.

CREATE INDEX nodes_by_parent ON nodes (space_id, parent_id, position);
CREATE INDEX nodes_parent_key ON nodes (parent_id);
CREATE INDEX invitations_node_key ON invitations (node_id);
