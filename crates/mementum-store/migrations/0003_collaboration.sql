-- Lot 3 : la version de l'espace (`spaces.lsn`, Ruling R133), les commentaires (R134), les
-- versions de page (R135) et les déplacements de blocs (R138).
--
-- Une seule migration pour le lot, qui peut changer jusqu'à sa clôture (Ruling R139) : aucun
-- binaire du lot n'ouvre d'ici là les données d'une instance que quelqu'un regarde.
--
-- `lsn` avance pour tout ce qui change ce que montre la barre latérale : tout ce que `rev` compte
-- (les entrées du calcul des accès), plus un renommage et un changement de place parmi les mêmes
-- frères, que `rev` ignore exprès (R83). Le serveur la pousse aux navigateurs, qui relisent alors
-- l'arbre.
--
-- Les tables neuves suivent leur page (`ON DELETE CASCADE`) : supprimer un nœud supprime ses fils
-- de commentaires (et, de proche en proche, leurs messages et leurs réactions), ses versions et
-- ses déplacements de blocs. Chaque clé suivie a un index qui commence par elle : sans lui,
-- SQLite relirait toute la table fille pour chaque ligne supprimée (voir `0002`).

ALTER TABLE spaces ADD COLUMN lsn INTEGER NOT NULL DEFAULT 0;

CREATE TABLE comment_threads (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    block_id TEXT,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    resolved_at INTEGER,
    resolved_by TEXT REFERENCES users(id)
);
CREATE INDEX comment_threads_by_node ON comment_threads (node_id, created_at);

CREATE TABLE comments (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL REFERENCES comment_threads(id) ON DELETE CASCADE,
    author_id TEXT NOT NULL REFERENCES users(id),
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    edited_at INTEGER
);
CREATE INDEX comments_by_thread ON comments (thread_id, created_at);

CREATE TABLE comment_reactions (
    comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    emoji TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (comment_id, user_id, emoji)
);

-- L'état (`state_blob`, quelques mégaoctets pour une grande page) vient en dernier : SQLite lit
-- les colonnes d'une ligne dans l'ordre, et une colonne placée après lui ferait lire tout l'état
-- à chaque liste des versions et à chaque purge (Ruling R191).
CREATE TABLE doc_versions (
    id TEXT PRIMARY KEY NOT NULL,
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('periodic', 'closing', 'before_restore')),
    authors TEXT NOT NULL,          -- JSON : identifiants des comptes, triés
    created_at INTEGER NOT NULL,
    state_blob BLOB NOT NULL
);
CREATE INDEX doc_versions_by_node ON doc_versions (node_id, created_at);

CREATE TABLE block_moves (
    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,           -- seq de la mise à jour qui a déplacé
    record BLOB NOT NULL,           -- écrit par `mementum-crdt` (Task 13)
    created_at INTEGER NOT NULL,
    PRIMARY KEY (node_id, seq)
);
