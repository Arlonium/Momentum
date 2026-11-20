-- Lot 4 : l'index de recherche (plan du lot 4, Rulings R901, R909, R918, R919 ; Task S1, R936 à
-- R939, R961 à R964, R977).
--
-- Une migration pour le lot (Ruling R139, amendé par R243) : `0004` est figée depuis le
-- 2026-11-20 (R936). Celle-ci peut changer jusqu'à la clôture du lot 4 : aucun binaire du lot
-- n'ouvre d'ici là les données d'une instance que quelqu'un regarde. Elle ne remplit qu'une chose :
-- chaque page existante est notée jamais indexée (`search_state`, `-1` ; R964), une instruction,
-- une ligne courte par page. Une page créée ou dupliquée l'est de même dans sa transaction
-- (R918) : aucune page n'est sans ligne, et le retard d'un espace se lit dans l'index partiel des
-- pages jamais indexées, sans parcourir ses pages. Le reste naît vide ; une instance existante est
-- indexée par le serveur, en fond (R915).
--
-- `instance_settings` garde, sous `search.committed` et `search.intent`, les deux jetons de
-- l'index plein texte (R909), et sous `search.projection`, la version de la projection que
-- l'index reflète (`PROJECTION_VERSION` de `mementum-search`, R960, R977).
--
-- `blocks_index` est la projection du document d'une page (conception §5.3) : une ligne par bloc
-- qui porte du texte, sous sa clé `(node_id, block_id)`, sans la colonne `path` de la conception
-- (R901). `hash` est l'empreinte du type, du texte et des liens du bloc : un bloc dont l'empreinte
-- n'a pas changé n'est pas récrit (R919). La clé reste deux identifiants en texte (R937) : la clé
-- étrangère vers `nodes(id)`, en texte, l'impose, et un identifiant de bloc est une chaîne du
-- client.
--
-- `page_links` dit qu'un bloc mène à une page (rétroliens) ; `target_id` est un UUID lu, sans clé
-- étrangère : la page visée peut être d'un autre espace, ou ne plus exister.
--
-- `search_state` dit où en est la projection de chaque page : le `seq` de la page que
-- `blocks_index` reflète, `-1` pour une page jamais indexée (le marqueur d'une page créée ou
-- dupliquée, R918, et chaque page d'une instance d'avant ce lot, R964), le `seq` où sa lecture a
-- échoué.
--
-- `search_outbox` dit ce que l'index plein texte n'a pas encore validé (R919) : un bloc changé ou
-- parti, ou la page entière (`block_id = ''` : son titre a changé, elle est supprimée, ou tout est
-- à reconstruire). Sans clé étrangère : la ligne d'une page supprimée lui survit, pour que ses
-- entrées quittent l'index plein texte. `token` est le jeton de l'intention qui l'a prise (R909) ;
-- toute écriture qui la touche de nouveau le retire (R939), si bien que la clôture d'une intention
-- ne retire jamais une ligne qui a bougé depuis.
--
-- Les trois premières tables partent avec leur page (`ON DELETE CASCADE`), comme `favorites`
-- (`0004`). Chaque clé que suit une suppression a un index qui commence par elle : leur clé
-- primaire.

CREATE TABLE blocks_index (
    node_id  TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    block_id TEXT NOT NULL,
    type     TEXT NOT NULL,
    text     TEXT NOT NULL,
    hash     BLOB NOT NULL,
    PRIMARY KEY (node_id, block_id)
) WITHOUT ROWID;

CREATE TABLE page_links (
    node_id   TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    block_id  TEXT NOT NULL,
    target_id TEXT NOT NULL,
    PRIMARY KEY (node_id, block_id, target_id)
) WITHOUT ROWID;
-- Les sources d'un rétrolien, dans l'ordre des identifiants, sans lire la table.
CREATE INDEX page_links_by_target ON page_links (target_id, node_id, block_id);

CREATE TABLE search_state (
    node_id     TEXT PRIMARY KEY NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    indexed_seq INTEGER NOT NULL,
    failed_seq  INTEGER,
    truncated   INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
-- Les pages jamais indexées (`Store::never_indexed`, `Store::behind_in_space`), sans parcourir
-- toutes les pages.
CREATE INDEX search_state_never ON search_state (node_id) WHERE indexed_seq = -1;
-- Chaque page existante : jamais indexée (R964).
INSERT INTO search_state (node_id, indexed_seq) SELECT id, -1 FROM nodes WHERE kind = 'page';

CREATE TABLE search_outbox (
    node_id  TEXT NOT NULL,
    block_id TEXT NOT NULL,
    token    TEXT,
    PRIMARY KEY (node_id, block_id)
) WITHOUT ROWID;
-- Les lignes prises par une intention : sa clôture ne parcourt pas tout ce qui attend (une
-- reconstruction y met toutes les pages de l'instance).
CREATE INDEX search_outbox_taken ON search_outbox (token) WHERE token IS NOT NULL;
