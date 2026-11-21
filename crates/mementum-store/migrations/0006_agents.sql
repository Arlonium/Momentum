-- Lot 5 : les agents (plan du lot 5, « La migration » ; Rulings R1218 à R1221, R1234, R1244,
-- R1246, R1249 ; Task S1, R1265 et suivants).
--
-- Une migration pour le lot (Ruling R139, amendé par R243) : `0005` est figée depuis la clôture du
-- lot 4 (R1596). Celle-ci peut changer jusqu'à la clôture du lot 5 : aucun binaire du lot n'ouvre
-- d'ici là les données d'une instance que quelqu'un regarde. Elle ne touche aucune table de `0005`
-- (R1249) : la date des pages est une table à elle, pas une colonne de `search_state`.
--
-- `api_tokens` : les jetons d'API personnels. Le store ne voit que le SHA-256 du jeton (`hash`),
-- et ses quatre derniers caractères pour qu'on le reconnaisse (`hint`, R1213). Deux niveaux, pas
-- d'accès complet (R1218). Une portée `all` ne nomme pas d'espace, une autre en nomme un ; celle
-- des pages les nomme dans `api_token_pages`, qu'une page supprimée quitte (cascade, R1219). Un
-- jeton révoqué ou échu reste, avec son journal, quatre-vingt-dix jours et cinquante par compte au
-- plus, purgés par la transaction qui crée ou révoque un jeton (R1220). `busy_count` et
-- `busy_last_at` comptent les appels refusés par le débit, qui n'écrivent aucune ligne ;
-- `calls_purged`, les lignes du journal que la purge a retirées (R1244).
--
-- `token_calls` : le journal d'activité, une ligne par appel qui a passé l'extracteur, écrite par
-- paquets ; ni texte cherché, ni MMD, ni texte remplacé (R1245). `space_id` et `node_id` sans clé :
-- l'objet peut partir, la ligne reste. La purge garde par jeton deux parts, les appels réussis et
-- les autres : d'où l'index, qui commence par le jeton et l'issue.
--
-- `page_edits` : la date du contenu de chaque page (R1246), que l'indexeur pose dans la transaction
-- où il indexe la page, et qui ne bouge que si le `seq` indexé avance (`edited_seq`). Elle est
-- remplie ici pour chaque page existante : la date et le compte de la dernière entrée de son
-- journal, sinon la date de son instantané, sinon celle de sa création, et le `seq` où en est la
-- page ; un dossier n'en a pas.
--
-- `doc_updates.token_id` : le jeton par lequel une entrée du journal a été écrite (R1234), sans clé
-- étrangère : un jeton purgé laisse son nom au journal.
--
-- `doc_versions` est rebâtie pour que son `CHECK` admette `before_agent` (R1234 ; SQLite ne change
-- pas un `CHECK` en place), et gagne `agent_authors` : ceux de ses auteurs qui ont écrit par un
-- jeton. L'état reste la dernière colonne (R191). Aucune table ni aucun déclencheur ne la vise ; la
-- copie est une instruction, puis l'ancienne part et la neuve prend son nom, puis son index revient
-- (le modèle des tables rebâties de `0002`).
--
-- Quand une ligne d'une table mère est supprimée, SQLite cherche ses lignes filles par leur clé
-- étrangère : `api_token_pages_by_node` sert à la suppression d'une page (la clé primaire commence
-- par le jeton), `api_tokens_by_space` à celle d'un espace (la cascade de `space_id`).

CREATE TABLE api_tokens (
    id           TEXT PRIMARY KEY NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    hash         BLOB NOT NULL UNIQUE,
    hint         TEXT NOT NULL,
    level        TEXT NOT NULL CHECK (level IN ('read', 'edit')),
    scope        TEXT NOT NULL CHECK (scope IN ('all', 'space', 'pages')),
    -- `NULL` pour une portée `all`, l'espace sinon.
    space_id     TEXT REFERENCES spaces(id) ON DELETE CASCADE,
    can_trash    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER,
    revoked_at   INTEGER,
    last_used_at INTEGER,
    busy_count   INTEGER NOT NULL DEFAULT 0,
    busy_last_at INTEGER,
    calls_purged INTEGER NOT NULL DEFAULT 0,
    CHECK ((scope = 'all') = (space_id IS NULL))
);
CREATE INDEX api_tokens_by_user ON api_tokens (user_id, created_at);
CREATE INDEX api_tokens_by_space ON api_tokens (space_id);

CREATE TABLE api_token_pages (
    token_id TEXT NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    node_id  TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    PRIMARY KEY (token_id, node_id)
) WITHOUT ROWID;
CREATE INDEX api_token_pages_by_node ON api_token_pages (node_id);

CREATE TABLE token_calls (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id        TEXT NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    at              INTEGER NOT NULL,
    surface         TEXT NOT NULL CHECK (surface IN ('mcp', 'rest')),
    -- L'outil, ou le nom de la route.
    tool            TEXT NOT NULL,
    space_id        TEXT,
    node_id         TEXT,
    outcome         TEXT NOT NULL CHECK (outcome IN ('ok', 'refused', 'error')),
    -- Le code du refus.
    code            TEXT,
    revision_before TEXT,
    revision_after  TEXT,
    blocks_touched  INTEGER
);
CREATE INDEX token_calls_by_token ON token_calls (token_id, outcome, id);

CREATE TABLE page_edits (
    node_id    TEXT PRIMARY KEY NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    edited_at  INTEGER NOT NULL,
    -- Un compte, sans clé ; vide quand la date n'est pas lue au journal.
    edited_by  TEXT,
    edited_seq INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO page_edits (node_id, edited_at, edited_by, edited_seq)
SELECT n.id,
       COALESCE(
           (SELECT u.created_at FROM doc_updates u WHERE u.node_id = n.id
            ORDER BY u.seq DESC LIMIT 1),
           (SELECT s.created_at FROM doc_snapshots s WHERE s.node_id = n.id),
           n.created_at),
       (SELECT u.actor_id FROM doc_updates u WHERE u.node_id = n.id ORDER BY u.seq DESC LIMIT 1),
       MAX(COALESCE((SELECT MAX(u.seq) FROM doc_updates u WHERE u.node_id = n.id), 0),
           COALESCE((SELECT s.seq FROM doc_snapshots s WHERE s.node_id = n.id), 0))
FROM nodes n WHERE n.kind = 'page';

ALTER TABLE doc_updates ADD COLUMN token_id TEXT;

CREATE TABLE doc_versions_new (
    id            TEXT PRIMARY KEY NOT NULL,
    node_id       TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    covers_seq    INTEGER NOT NULL,
    reason        TEXT NOT NULL
                  CHECK (reason IN ('periodic', 'closing', 'before_restore', 'before_agent')),
    -- JSON : identifiants des comptes, triés.
    authors       TEXT NOT NULL,
    -- JSON : ceux d'entre eux qui ont écrit par un jeton, triés.
    agent_authors TEXT NOT NULL DEFAULT '[]',
    created_at    INTEGER NOT NULL,
    state_blob    BLOB NOT NULL
);
INSERT INTO doc_versions_new (id, node_id, seq, covers_seq, reason, authors, created_at, state_blob)
SELECT id, node_id, seq, covers_seq, reason, authors, created_at, state_blob FROM doc_versions;
DROP TABLE doc_versions;
ALTER TABLE doc_versions_new RENAME TO doc_versions;
CREATE INDEX doc_versions_by_node ON doc_versions (node_id, seq, created_at);
