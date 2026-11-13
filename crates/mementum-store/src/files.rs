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
    pub async fn attach_file(
        &self,
        new: NewAttachment<'_>,
        now: i64,
    ) -> Result<Attachment, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO files (hash, size, mime, created_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT (hash) DO NOTHING",
        )
        .bind(&new.hash[..])
        .bind(new.size)
        .bind(new.mime)
        .bind(now)
        .execute(&mut *tx)
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
    /// passe pas entre les deux. `None`, sans rien écrire, si la source n'est pas de `from_node` ;
    /// `NotFound`, sans rien écrire, si `to_node` ou le compte `by` n'existe pas.
    pub async fn copy_attachment(
        &self,
        from_node: Uuid,
        from_id: Uuid,
        id: Uuid,
        to_node: Uuid,
        by: Uuid,
        now: i64,
    ) -> Result<Option<Attachment>, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
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
    /// deux garde son contenu.
    pub async fn sweep_files(&self) -> Result<Vec<[u8; 32]>, StoreError> {
        let candidates: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT hash FROM files \
             WHERE NOT EXISTS (SELECT 1 FROM attachments a WHERE a.file_hash = files.hash)",
        )
        .fetch_all(&self.pool)
        .await?;
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        // Les candidats en un seul paramètre, un tableau JSON de chaînes hexadécimales : des
        // octets relus de la base, sans rien à échapper.
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
        .fetch_all(&self.pool)
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
