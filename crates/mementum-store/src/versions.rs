//! Versions d'une page (Ruling R135) : l'état Yjs complet d'une page à un instant, avec les
//! comptes qui l'ont modifiée depuis la version d'avant.
//!
//! C'est l'acteur de la page (`mementum-sync`) qui décide quand en couper une, et qui encode
//! l'état depuis son document : le store ne garde jamais d'octets reçus d'un client. Il garde
//! l'état tel qu'on le lui donne, et le rend tel quel. Une page garde ses [`MAX_VERSIONS`]
//! dernières versions, dont au plus [`MAX_BEFORE_RESTORE`] `before_restore` (Ruling R189) : la
//! purge retire d'abord les plus anciennes de cette sorte au-delà de vingt, puis les plus
//! anciennes de toutes au-delà de cent, dans la transaction qui écrit la version neuve, si bien
//! qu'une version refusée n'efface rien. Des restaurations répétées ne font donc jamais partir
//! plus de vingt versions ordinaires. Elles suivent leur page (`ON DELETE CASCADE`, migration
//! `0003`) ; la compaction, qui purge le journal, n'y touche pas.
//!
//! **Ordre.** La plus récente d'abord : par `seq` (le journal de la page qu'elle couvre, qui ne
//! fait que croître), puis par date d'écriture, puis par identifiant (UUIDv7, croissant dans le
//! processus). Le `seq` d'abord : une horloge système qui recule ne fait ni purger la version
//! qu'on vient d'écrire, ni la ranger après les autres.
//!
//! **Auteurs.** Des identifiants de comptes, triés et sans doublon, en JSON (`["…","…"]`) : ce
//! sont des UUID relus, sans rien à échapper. Ils se relisent avec le nom que le store connaît du
//! compte, qu'il soit encore membre de l'espace ou non ; un identifiant inconnu (aucune clé
//! étrangère ne le garde) se relit avec un nom vide.
//!
//! Une version n'avance ni `rev` ni `lsn` : elle ne change ni les droits ni ce que montre la
//! barre latérale. Ses transactions ne sont pas des `WriteTx`.

use std::collections::HashMap;

use sqlx::Row;
use uuid::Uuid;

use crate::{Author, Store, StoreError, now_ms, parse_uuid, unreadable};

/// Versions gardées par page : au-delà, les plus anciennes partent.
pub const MAX_VERSIONS: usize = 100;

/// Versions `before_restore` gardées par page : au-delà, les plus anciennes de cette sorte
/// partent d'abord, avant le plafond de [`MAX_VERSIONS`] (Ruling R189).
pub const MAX_BEFORE_RESTORE: usize = 20;

/// Pourquoi une version a été coupée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionReason {
    /// Avant l'écriture qui suit de l'intervalle réglé la première écriture non versionnée.
    Periodic,
    /// À l'arrêt de l'acteur de la page, s'il restait des écritures non versionnées.
    Closing,
    /// Juste avant une restauration : ce qu'elle remplace.
    BeforeRestore,
}

impl VersionReason {
    /// Le nom écrit en base (`doc_versions.reason`).
    pub fn as_str(self) -> &'static str {
        match self {
            VersionReason::Periodic => "periodic",
            VersionReason::Closing => "closing",
            VersionReason::BeforeRestore => "before_restore",
        }
    }

    fn parse(text: &str) -> Option<VersionReason> {
        [
            VersionReason::Periodic,
            VersionReason::Closing,
            VersionReason::BeforeRestore,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == text)
    }
}

/// Une version, sans son état ([`Store::get_version`] le rend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub id: Uuid,
    /// Le `seq` du journal que l'état couvre.
    pub seq: i64,
    pub reason: VersionReason,
    /// Les comptes qui ont modifié la page depuis la version d'avant, dans l'ordre de leurs
    /// identifiants.
    pub authors: Vec<Author>,
    /// Millisecondes Unix.
    pub created_at: i64,
}

/// Ce que le journal garde des écritures faites après la dernière version d'une page
/// ([`Store::unversioned`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unversioned {
    /// Millisecondes Unix : la plus ancienne écriture connue au-delà de la dernière version ;
    /// `None` : aucune.
    pub since: Option<i64>,
    /// Les comptes de ces écritures, triés, pour ce qu'il en reste au journal.
    pub authors: Vec<Uuid>,
}

impl Store {
    /// Garde une version de la page : son état Yjs (`state`, écrit par le serveur), le `seq` du
    /// journal qu'il couvre, les comptes qui l'ont modifiée depuis la version d'avant (triés et
    /// dédoublonnés ici) et la raison. Dans la même transaction, seules les [`MAX_VERSIONS`]
    /// dernières versions de la page restent, dont au plus [`MAX_BEFORE_RESTORE`]
    /// `before_restore`. `NotFound` si la page n'existe pas.
    pub async fn put_version(
        &self,
        node_id: Uuid,
        seq: i64,
        state: &[u8],
        authors: &[Uuid],
        reason: VersionReason,
    ) -> Result<Uuid, StoreError> {
        let mut sorted = authors.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        let authors = format!(
            "[{}]",
            sorted
                .iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let (id, node) = (Uuid::now_v7(), node_id.to_string());
        // IMMEDIATE : on prend le verrou d'écriture d'emblée, sans montée de verrou qui échouerait.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO doc_versions (id, node_id, seq, state_blob, reason, authors, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&node)
        .bind(seq)
        .bind(state)
        .bind(reason.as_str())
        .bind(&authors)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?;
        // Les `before_restore` au-delà des vingt plus récentes d'abord (Ruling R189), puis toutes
        // au-delà des cent plus récentes.
        sqlx::query(
            "DELETE FROM doc_versions WHERE node_id = ?1 AND reason = ?2 AND id NOT IN ( \
                 SELECT id FROM doc_versions WHERE node_id = ?1 AND reason = ?2 \
                 ORDER BY seq DESC, created_at DESC, id DESC LIMIT ?3)",
        )
        .bind(&node)
        .bind(VersionReason::BeforeRestore.as_str())
        .bind(MAX_BEFORE_RESTORE as i64)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM doc_versions WHERE node_id = ?1 AND id NOT IN ( \
                 SELECT id FROM doc_versions WHERE node_id = ?1 \
                 ORDER BY seq DESC, created_at DESC, id DESC LIMIT ?2)",
        )
        .bind(&node)
        .bind(MAX_VERSIONS as i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Les versions de la page, la plus récente d'abord, sans leur état, lues en une
    /// transaction. Une page sans version, ou qui n'existe pas : aucune.
    pub async fn list_versions(&self, node_id: Uuid) -> Result<Vec<Version>, StoreError> {
        let node = node_id.to_string();
        let mut tx = self.pool.begin().await?;
        let mut versions = sqlx::query(
            "SELECT id, seq, reason, created_at FROM doc_versions WHERE node_id = ? \
             ORDER BY seq DESC, created_at DESC, id DESC",
        )
        .bind(&node)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|r| {
            let reason: String = r.get("reason");
            Ok(Version {
                id: parse_uuid(&r.get::<String, _>("id"))?,
                seq: r.get("seq"),
                reason: VersionReason::parse(&reason)
                    .ok_or_else(|| unreadable("raison de version", &reason))?,
                authors: Vec::new(),
                created_at: r.get("created_at"),
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
        // Les auteurs dans l'ordre où la version les garde (triés à l'écriture).
        let authors = sqlx::query(
            "SELECT v.id AS version_id, a.value AS author_id, u.name AS author_name \
             FROM doc_versions v, json_each(v.authors) a LEFT JOIN users u ON u.id = a.value \
             WHERE v.node_id = ? ORDER BY v.id, a.key",
        )
        .bind(&node)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let index: HashMap<Uuid, usize> = versions
            .iter()
            .enumerate()
            .map(|(i, version)| (version.id, i))
            .collect();
        for r in &authors {
            let version = parse_uuid(&r.get::<String, _>("version_id"))?;
            // Lus dans la même transaction que les versions : chacun a la sienne.
            let Some(&at) = index.get(&version) else {
                continue;
            };
            versions[at].authors.push(Author {
                id: parse_uuid(&r.get::<String, _>("author_id"))?,
                name: r
                    .get::<Option<String>, _>("author_name")
                    .unwrap_or_default(),
            });
        }
        Ok(versions)
    }

    /// L'état gardé de la version `id`, tel qu'il a été écrit, si elle est de cette page ;
    /// `None` si elle est d'une autre page, ou n'existe pas (la même réponse).
    pub async fn get_version(
        &self,
        node_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let row = sqlx::query("SELECT state_blob FROM doc_versions WHERE id = ? AND node_id = ?")
            .bind(id.to_string())
            .bind(node_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get(0)))
    }

    /// Ce que le journal garde des écritures faites après la dernière version de la page
    /// (celle de plus grand `seq` ; sans version, depuis la création), lu en une transaction :
    /// de quoi reprendre la tranche en cours quand l'acteur de la page redémarre (après un arrêt
    /// brutal, la version `closing` n'a pas été coupée).
    ///
    /// La compaction purge le journal et ses auteurs : si l'instantané couvre des écritures
    /// au-delà de la dernière version, la tranche date au plus tard de lui (sa date est
    /// postérieure aux écritures purgées), et ses auteurs ne sont plus connus. Le journal ne
    /// porte que des écritures qui ont changé la page : une mise à jour de client sans effet
    /// n'y est pas écrite (Ruling R181).
    pub async fn unversioned(&self, node_id: Uuid) -> Result<Unversioned, StoreError> {
        let node = node_id.to_string();
        let mut tx = self.pool.begin().await?;
        let versioned: i64 =
            sqlx::query("SELECT COALESCE(MAX(seq), 0) FROM doc_versions WHERE node_id = ?")
                .bind(&node)
                .fetch_one(&mut *tx)
                .await?
                .get(0);
        let snapshot = sqlx::query("SELECT seq, created_at FROM doc_snapshots WHERE node_id = ?")
            .bind(&node)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| (r.get::<i64, _>(0), r.get::<i64, _>(1)));
        let journal: Option<i64> =
            sqlx::query("SELECT MIN(created_at) FROM doc_updates WHERE node_id = ? AND seq > ?")
                .bind(&node)
                .bind(versioned)
                .fetch_one(&mut *tx)
                .await?
                .get(0);
        let authors = sqlx::query(
            "SELECT DISTINCT actor_id FROM doc_updates \
             WHERE node_id = ? AND seq > ? AND actor_id IS NOT NULL ORDER BY actor_id",
        )
        .bind(&node)
        .bind(versioned)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|r| parse_uuid(&r.get::<String, _>(0)))
        .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        let compacted = snapshot
            .filter(|(seq, _)| *seq > versioned)
            .map(|(_, at)| at);
        let since = match (compacted, journal) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Ok(Unversioned { since, authors })
    }
}
