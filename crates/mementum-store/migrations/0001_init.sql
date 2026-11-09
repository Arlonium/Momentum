-- Schéma initial du lot 1 : organisations, coffres, arbre de pages, journal Yjs, instantanés.

CREATE TABLE instance_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE orgs (
    id         TEXT PRIMARY KEY,
    slug       TEXT UNIQUE NOT NULL,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE vaults (
    id         TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL REFERENCES orgs(id),
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('personal','shared')),
    created_at INTEGER NOT NULL
);

CREATE TABLE nodes (
    id         TEXT PRIMARY KEY,
    org_id     TEXT NOT NULL REFERENCES orgs(id),
    vault_id   TEXT NOT NULL REFERENCES vaults(id),
    parent_id  TEXT REFERENCES nodes(id),
    kind       TEXT NOT NULL CHECK (kind IN ('folder','page')),
    title      TEXT NOT NULL,
    position   TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    trashed_at INTEGER
);

CREATE INDEX nodes_by_parent ON nodes (vault_id, parent_id, position);

CREATE TABLE doc_updates (
    node_id     TEXT NOT NULL REFERENCES nodes(id),
    seq         INTEGER NOT NULL,
    update_blob BLOB NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (node_id, seq)
);

CREATE TABLE doc_snapshots (
    node_id    TEXT PRIMARY KEY REFERENCES nodes(id),
    seq        INTEGER NOT NULL,
    state_blob BLOB NOT NULL,
    created_at INTEGER NOT NULL
);
