//! Les fichiers d'une page (Rulings R244, R245) : un **contenu** (`files`), rangé sur le disque
//! sous son SHA-256 et partagé par toutes les attaches qui le nomment, et des **attaches**
//! (`attachments`), chacune disant qu'une page porte ce contenu, sous un nom d'affichage. C'est
//! l'attache que nomme l'adresse d'un fichier (`/api/v1/pages/<page>/files/<attache>`).
//!
//! Le store ne touche pas au disque : c'est le dépôt du serveur qui y écrit et y lit les
//! contenus, sous leur hash. Il ne juge ni le niveau de la personne sur une page (`access.rs`,
//! côté serveur) ni la corbeille. Il décide, dans la transaction qui écrit, ce qui ne se décide
//! qu'avec l'écriture : la page existe encore (sinon [`StoreError::NotFound`], et rien n'est
//! écrit, pas même la ligne du contenu) ; l'attache recopiée est bien de la page qu'on nomme, et
//! son contenu ne part pas entre la lecture et la copie.
//!
//! **Un contenu n'est jamais récrit** (Ruling R267) : sa taille et son type découlent de ses
//! octets, que nomme son hash ; une attache d'un contenu déjà connu rend la taille et le type
//! gardés, pas ceux qu'on lui donne. Le type ne changerait que par une migration qui relirait
//! les contenus, le jour où la table des types du serveur change (R246).
//!
//! **Durée de vie** (R245). Une attache vit autant que sa page : elle part avec elle
//! (`ON DELETE CASCADE`, migration `0004`), jamais avec un bloc. Un contenu reste tant qu'une
//! attache le nomme ; [`Store::sweep_files`] retire de la base ceux qu'aucune ne nomme plus, et
//! rend leurs hash, pour que le serveur les retire du disque, sous le verrou du dépôt qui ordonne
//! aussi « poser un contenu puis l'attacher ».
//!
//! Une attache n'avance ni `rev` ni `lsn` : elle ne change ni les droits ni ce que montre la
//! barre latérale. Ses transactions ne sont donc pas des `WriteTx`, et n'annoncent rien.

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

use crate::queue::Lane;
use crate::{Store, StoreError, parse_uuid, unreadable};

/// Une attache : la page qui porte le fichier, son contenu et son nom d'affichage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: Uuid,
    pub node_id: Uuid,
    /// SHA-256 du contenu.
    pub hash: [u8; 32],
    /// Octets du contenu.
    pub size: i64,
    /// Le type du contenu, lu par le serveur dans ses premiers octets (R246).
    pub mime: String,
    /// Le nom d'affichage, tel que le serveur l'a nettoyé : un libellé, jamais un chemin.
    pub name: String,
}

/// Une attache à écrire ([`Store::attach_file`]) : la ligne du contenu, s'il est neuf, prend
/// `size` et `mime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewAttachment<'a> {
    pub id: Uuid,
    pub node_id: Uuid,
    pub hash: [u8; 32],
    pub size: i64,
    pub mime: &'a str,
    pub name: &'a str,
    pub created_by: Uuid,
}

/// Une attache et son contenu, par les noms de colonnes de [`attachment_from_row`], en littéral
/// pour que le SQL reste statique (`concat!`).
macro_rules! attachment_select {
    () => {
        "SELECT a.id, a.node_id, a.file_hash, f.size, f.mime, a.name \
         FROM attachments a JOIN files f ON f.hash = a.file_hash"
    };
}

/// Relit un hash stocké en `BLOB` : 32 octets, sinon une erreur, jamais une ligne sautée.
fn hash_from(bytes: Vec<u8>) -> Result<[u8; 32], StoreError> {
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| unreadable("hash de contenu", &format!("{} octets", bytes.len())))
}

fn attachment_from_row(r: &SqliteRow) -> Result<Attachment, StoreError> {
    Ok(Attachment {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        node_id: parse_uuid(&r.get::<String, _>("node_id"))?,
        hash: hash_from(r.get("file_hash"))?,
        size: r.get("size"),
        mime: r.get("mime"),
        name: r.get("name"),
    })
}

/// L'attache `id` de la page `node_id`, avec son contenu ; `None` si elle n'est pas de cette page.
async fn read_attachment(
    conn: &mut SqliteConnection,
    node_id: Uuid,
    id: Uuid,
) -> Result<Option<Attachment>, StoreError> {
    sqlx::query(concat!(
        attachment_select!(),
        " WHERE a.id = ? AND a.node_id = ?"
    ))
    .bind(id.to_string())
    .bind(node_id.to_string())
    .fetch_optional(conn)
    .await?
    .as_ref()
    .map(attachment_from_row)
    .transpose()
}

/// La page `id` est-elle à la corbeille, elle-même ou par une page parente ? `None` si elle
/// n'existe pas. La remontée va de parent en parent et s'arrête à la première page à la corbeille
/// (`UNION` : aussi si des parents tournaient en rond), comme `Store::trashed_since`.
async fn in_trash(conn: &mut SqliteConnection, id: Uuid) -> Result<Option<bool>, StoreError> {
    let id = id.to_string();
    let exists = sqlx::query("SELECT 1 FROM nodes WHERE id = ?")
        .bind(&id)
        .fetch_optional(&mut *conn)
        .await?;
    if exists.is_none() {
        return Ok(None);
    }
    let trashed = sqlx::query(
        "WITH RECURSIVE anc(id, parent_id, trashed_at) AS ( \
             SELECT id, parent_id, trashed_at FROM nodes WHERE id = ?1 \
             UNION \
             SELECT n.id, n.parent_id, n.trashed_at FROM nodes n \
             JOIN anc ON n.id = anc.parent_id \
             WHERE anc.trashed_at IS NULL \
         ) SELECT 1 FROM anc WHERE trashed_at IS NOT NULL LIMIT 1",
    )
    .bind(&id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(Some(trashed.is_some()))
}

/// Écrit une attache de `hash` sur la page `node_id`, dans la transaction de l'appelant (la
/// duplication d'une page s'en sert dans la sienne). `NotFound` si la page, le compte ou la ligne
/// du contenu n'existe pas : c'est la base qui le dit, par ses clés étrangères.
pub(crate) async fn insert_attachment(
    conn: &mut SqliteConnection,
    id: Uuid,
    node_id: Uuid,
    hash: &[u8; 32],
    name: &str,
    created_by: Uuid,
    now: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO attachments (id, node_id, file_hash, name, created_by, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(node_id.to_string())
    .bind(&hash[..])
    .bind(name)
    .bind(created_by.to_string())
    .bind(now)
    .execute(conn)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(())
}

impl Store {
    /// Attache un fichier à une page : la ligne du contenu, s'il est neuf, et l'attache, en une
    /// transaction. Un contenu déjà connu n'est pas récrit : l'attache rendue porte sa taille et
    /// son type gardés (R267). `NotFound` si la page ou le compte n'existe pas, et rien n'est
    /// écrit, pas même la ligne du contenu.
    ///
    /// Elle s'écrit dans la voie du dépôt, pas dans celle de l'auteur (Ruling R522) : l'envoi tient
    /// le verrou du dépôt, commun à l'instance, jusqu'à l'attache écrite ; dans la voie de
    /// l'auteur, elle attendrait toutes les écritures qu'il a en file, et tous les envois avec elle.
    pub async fn attach_file(
        &self,
        new: NewAttachment<'_>,
        now: i64,
    ) -> Result<Attachment, StoreError> {
        let mut tx = self.write_in(Lane::Files).await?;
        sqlx::query(
            "INSERT INTO files (hash, size, mime, created_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT (hash) DO NOTHING",
        )
        .bind(&new.hash[..])
        .bind(new.size)
        .bind(new.mime)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        insert_attachment(
            &mut tx,
            new.id,
            new.node_id,
            &new.hash,
            new.name,
            new.created_by,
            now,
        )
        .await?;
        let attachment = read_attachment(&mut tx, new.node_id, new.id)
            .await?
            .ok_or(StoreError::NotFound)?;
        tx.commit().await?;
        Ok(attachment)
    }

    /// L'attache `id` de la page `node_id` ; `None` si elle n'est pas de cette page (qu'elle
    /// soit d'une autre ou d'aucune).
    pub async fn attachment(
        &self,
        node_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Attachment>, StoreError> {
        let mut conn = self.pool.acquire().await?;
        read_attachment(&mut conn, node_id, id).await
    }

    /// Recopie l'attache `from_id` de la page `from_node` dans la page `to_node` : une attache
    /// neuve, d'identifiant `id`, au même contenu et au même nom, sans un octet renvoyé (R249).
    /// La source est lue et la copie écrite dans la même transaction d'écriture : un ménage ne
    /// passe pas entre les deux. Dans cette transaction, et dans cet ordre, comme les refus de la
    /// route : `NotFound` si `to_node` n'existe plus ; `Trashed` si `to_node` est à la corbeille,
    /// elle-même ou par une page parente (Ruling R577 : la route l'a vu hors de la corbeille, mais
    /// la recopie a pu attendre son tour longtemps) ; `None` si la source n'est pas de
    /// `from_node` ; `NotFound` encore si le compte `by` n'existe pas. Aucun de ces cas n'écrit
    /// rien. Elle ne crée jamais de ligne `files` : la source attachée, relue ici, garde la
    /// sienne, et la clé étrangère refuserait l'attache d'un contenu sans ligne.
    ///
    /// Elle s'écrit dans la voie du compte de la tâche, pas dans celle du dépôt (Ruling R522) :
    /// elle ne se fait pas sous le verrou du dépôt, et la voie du dépôt, qui passe en priorité, n'a
    /// jamais qu'une écriture à la fois ; des recopies y passeraient devant toutes les voies, et
    /// devant les attaches des envois (`copies_wait_in_the_lane_of_their_account`).
    pub async fn copy_attachment(
        &self,
        from_node: Uuid,
        from_id: Uuid,
        id: Uuid,
        to_node: Uuid,
        by: Uuid,
        now: i64,
    ) -> Result<Option<Attachment>, StoreError> {
        let mut tx = self.write().await?;
        match in_trash(&mut tx, to_node).await? {
            None => return Err(StoreError::NotFound),
            Some(true) => return Err(StoreError::Trashed),
            Some(false) => {}
        }
        let Some(source) = read_attachment(&mut tx, from_node, from_id).await? else {
            return Ok(None);
        };
        insert_attachment(&mut tx, id, to_node, &source.hash, &source.name, by, now).await?;
        tx.commit().await?;
        Ok(Some(Attachment {
            id,
            node_id: to_node,
            ..source
        }))
    }

    /// Le ménage (R245) : supprime les lignes des contenus qu'aucune attache ne nomme plus, et
    /// rend leurs hash, chacun une fois, pour que le serveur les retire du disque. Il ne regarde
    /// ni les blocs ni les versions : un contenu attaché à une page reste, même si plus aucun
    /// bloc ne le nomme.
    ///
    /// Il relit tous les contenus de l'instance (R270), d'abord sans écrire : une lecture ne
    /// prend pas le verrou d'écriture de SQLite, unique pour toute l'instance, et le plus souvent
    /// il n'y a rien à retirer. Seuls les candidats trouvés sont ensuite supprimés, en une
    /// instruction qui revérifie qu'aucune attache ne les nomme : une attache écrite entre les
    /// deux garde son contenu. C'est [`Store::unattached_files`] puis
    /// [`Store::forget_unattached`] ; le serveur mène les deux temps à part, pour ne prendre le
    /// verrou du dépôt qu'au second (Ruling R555).
    ///
    /// **Réservé aux tests** (fonction `test-hooks`, Ruling R576) : il retire des lignes sans le
    /// verrou du dépôt, et un appelant qui retirerait ensuite les contenus du disque sous un garde
    /// pris après perdrait le fichier d'un envoi du même contenu. Le serveur passe par
    /// `FileStore::forget_and_remove`.
    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub async fn sweep_files(&self) -> Result<Vec<[u8; 32]>, StoreError> {
        let candidates = self.unattached_files().await?;
        self.forget_unattached(&candidates).await
    }

    /// Le premier temps du ménage : les hash des contenus qu'aucune attache ne nomme, lus sans
    /// rien écrire ni attendre aucun tour (R270 : tous les contenus de l'instance sont relus, par
    /// l'index `attachments_by_file`). Une liste de candidats, déjà peut-être périmée quand elle
    /// rend : seul [`Store::forget_unattached`] décide, dans la transaction qui écrit.
    pub async fn unattached_files(&self) -> Result<Vec<[u8; 32]>, StoreError> {
        let candidates: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT hash FROM files \
             WHERE NOT EXISTS (SELECT 1 FROM attachments a WHERE a.file_hash = files.hash)",
        )
        .fetch_all(&self.pool)
        .await?;
        candidates.into_iter().map(hash_from).collect()
    }

    /// Le second temps du ménage : supprime, parmi `candidates`, les lignes des contenus
    /// qu'aucune attache ne nomme **encore**, en une instruction qui le revérifie, et rend leurs
    /// hash, chacun une fois : ceux-là, et eux seuls, sont à retirer du disque. Un contenu attaché
    /// depuis la lecture des candidats reste ; un contenu absent de la liste n'est pas regardé.
    /// Une liste vide n'écrit rien, et n'attend aucun tour (R270).
    ///
    /// Il s'écrit dans la voie du dépôt (Ruling R522) : le serveur le mène sous le verrou du
    /// dépôt, commun à l'instance, que tous les envois attendent ; dans la voie du compte qui
    /// supprime la page, il attendrait toutes les écritures que ce compte a en file.
    ///
    /// **Réservé au dépôt du serveur** (Ruling R576) : `FileStore::forget_and_remove`, qui vérifie
    /// le garde du verrou du dépôt avant de l'appeler, est son seul appelant hors des tests (un test
    /// du serveur le tient, `only_the_depot_forgets_files`). Mené hors de ce verrou, il laisserait
    /// un envoi trouver « déjà là » un contenu dont la ligne vient de partir.
    #[doc(hidden)]
    pub async fn forget_unattached(
        &self,
        candidates: &[[u8; 32]],
    ) -> Result<Vec<[u8; 32]>, StoreError> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        // Les candidats en un seul paramètre, un tableau JSON de chaînes hexadécimales : des
        // octets de 32, sans rien à échapper.
        let list = format!(
            "[{}]",
            candidates
                .iter()
                .map(|hash| {
                    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
                    format!("\"{hex}\"")
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        sqlx::query(
            "DELETE FROM files \
             WHERE hash IN (SELECT unhex(value) FROM json_each(?)) \
               AND NOT EXISTS (SELECT 1 FROM attachments a WHERE a.file_hash = files.hash) \
             RETURNING hash",
        )
        .bind(list)
        .fetch_all(&mut *self.write_conn_in(Lane::Files).await?)
        .await?
        .into_iter()
        .map(|r| hash_from(r.get(0)))
        .collect()
    }

    /// Les hash de tous les contenus que la base connaît, dans l'ordre de leurs octets : ce que
    /// le serveur compare au dépôt au démarrage, pour journaliser ceux qu'elle ne connaît pas.
    pub async fn known_files(&self) -> Result<Vec<[u8; 32]>, StoreError> {
        sqlx::query("SELECT hash FROM files ORDER BY hash")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|r| hash_from(r.get(0)))
            .collect()
    }
}
