//! Les favoris d'un compte (Ruling R254) : une ligne `(compte, page)`, qui part avec sa page
//! (`ON DELETE CASCADE`, migration `0004`).
//!
//! Le store ne juge pas le niveau de la personne sur la page, ni la corbeille : il rend les
//! favoris du compte dans un espace tels qu'ils sont en base, pages à la corbeille comprises, et
//! c'est la route qui ne montre que ce que `access.rs` montre au compte. Une liste filtrée ainsi
//! ne nomme jamais une page devenue invisible, et la rend quand l'accès revient, à sa place.
//!
//! **Un favori n'avance ni `rev` ni `lsn`** (R254) : il ne change ni les droits ni ce que montre
//! la barre latérale des autres, seulement celle de son auteur. `lsn` fait relire l'arbre à tous
//! les onglets de l'espace : l'avancer donnerait au compte le moins doté le moyen de faire
//! relire l'arbre à tout le monde, et compterait des gestes privés dans un numéro que tous
//! reçoivent. Ses écritures ne sont donc pas des `WriteTx`, et n'annoncent rien.

use sqlx::Row;
use uuid::Uuid;

use crate::{Store, StoreError, parse_uuid};

impl Store {
    /// Pose (`on`) ou retire le favori `(user_id, node_id)`. Rend `true` si quelque chose a
    /// changé ; un favori déjà posé, ou déjà absent, n'est pas récrit (ni sa date, ni sa place
    /// dans la liste) et rend `false`. `NotFound`, sans rien écrire, pour une page inconnue (et,
    /// pour poser, un compte inconnu). Ni `rev` ni `lsn` ne bougent, rien n'est annoncé (R254).
    ///
    /// **Il lit d'abord, et n'écrit que s'il y a quelque chose à changer** (Ruling R283, la règle
    /// de R270). Le verrou d'écriture de SQLite est unique pour l'instance, et un appel sans effet
    /// le prenait, le temps de ne rien écrire : un simple lecteur qui redemande ce qu'il a déjà
    /// faisait attendre le journal des pages ouvertes. La lecture n'est qu'un raccourci, jamais
    /// la preuve : l'écriture garde ses propres gardes, pour ce qui change entre les deux (le
    /// conflit laisse tel quel un favori posé entre-temps ; un retrait qui ne retire rien relit
    /// la page).
    pub async fn set_favorite(
        &self,
        user_id: Uuid,
        node_id: Uuid,
        on: bool,
        now: i64,
    ) -> Result<bool, StoreError> {
        let (user, node) = (user_id.to_string(), node_id.to_string());
        let posed = sqlx::query("SELECT 1 FROM favorites WHERE user_id = ? AND node_id = ?")
            .bind(&user)
            .bind(&node)
            .fetch_optional(&self.pool)
            .await?
            .is_some();
        if on {
            if posed {
                // Sa ligne dit que la page et le compte existent.
                return Ok(false);
            }
            // Une page ou un compte inconnu : la clé étrangère le dit ; un favori posé depuis
            // la lecture : le conflit le laisse tel quel.
            let added = sqlx::query(
                "INSERT INTO favorites (user_id, node_id, created_at) VALUES (?, ?, ?) \
                 ON CONFLICT (user_id, node_id) DO NOTHING",
            )
            .bind(&user)
            .bind(&node)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from_fk)?
            .rows_affected();
            return Ok(added > 0);
        }
        if posed {
            let removed = sqlx::query("DELETE FROM favorites WHERE user_id = ? AND node_id = ?")
                .bind(&user)
                .bind(&node)
                .execute(&self.pool)
                .await?
                .rows_affected();
            if removed > 0 {
                return Ok(true);
            }
        }
        // Rien à retirer, ou rien de retiré (parti depuis la lecture, avec sa page peut-être) :
        // un favori absent d'une page qui existe, ou une page inconnue.
        sqlx::query("SELECT 1 FROM nodes WHERE id = ?")
            .bind(&node)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)?;
        Ok(false)
    }

    /// Les pages favorites du compte dans l'espace, dans l'ordre où il les a ajoutées (à la même
    /// date, par identifiant), pages à la corbeille comprises : la route ne garde que celles que
    /// le compte voit, hors corbeille.
    pub async fn favorites(&self, user_id: Uuid, space_id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        sqlx::query(
            "SELECT f.node_id FROM favorites f JOIN nodes n ON n.id = f.node_id \
             WHERE f.user_id = ? AND n.space_id = ? ORDER BY f.created_at, f.node_id",
        )
        .bind(user_id.to_string())
        .bind(space_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|r| parse_uuid(&r.get::<String, _>(0)))
        .collect()
    }
}
