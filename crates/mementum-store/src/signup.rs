//! Inscription : un compte et ce qui vient avec lui, en une seule transaction.
//!
//! Une inscription écrit dans `users`, puis dans `members`, `spaces`, `invitations` ou `acl`,
//! et enfin dans `sessions` (la sienne, et celle qu'elle remplace).
//! En plusieurs transactions, un arrêt ou une erreur de la base au milieu laisserait un compte
//! sans espace, dont l'adresse est prise ; et sur une base du lot 1, des pages sans propriétaire
//! pour de bon, puisque seul le premier compte reprend les espaces sans membre, et à son
//! inscription seulement. [`Store::sign_up`] écrit donc tout, ou rien. Il ne refait la logique
//! d'aucune table : il enchaîne, dans sa transaction, les fonctions de `users.rs`, `members.rs`,
//! `invitations.rs` et `spaces.rs`.

use mementum_acl::Role;
use uuid::Uuid;

use crate::invitations::{Invitation, accept, acceptable};
use crate::members::{adopt_orphans, raise_role, spaces_of_in};
use crate::spaces::{Space, WriteTx, bump_rev, insert_space};
use crate::users::{NewSession, NewUser, User, insert_session, insert_user, remove_session};
use crate::{Store, StoreError};

/// Ce qu'une inscription a écrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedUp {
    /// Le compte créé ; admin de l'instance s'il en est le premier.
    pub user: User,
    /// Les espaces sans membre que le compte a repris, par date de création, à leur révision
    /// nouvelle. Vide, sauf pour le premier compte d'une instance qui en avait (une base du
    /// lot 1).
    pub adopted: Vec<Space>,
    /// L'invitation acceptée : un jeton était donné, et elle valait encore.
    pub accepted: Option<Invitation>,
    /// Un jeton était donné, mais son invitation ne valait plus à l'instant de l'écriture
    /// (échue, révoquée, déjà acceptée, auteur qui a perdu son rôle) : le compte existe, sans
    /// elle, donc sans espace ni droit.
    pub invitation_withdrawn: bool,
    /// L'espace créé pour le compte, dont il est `owner` : sans jeton, quand il n'a repris
    /// aucun espace.
    pub space: Option<Space>,
    /// Les espaces où le compte a un rôle, avec ce rôle, par date de création, lus dans la
    /// transaction : de quoi répondre sans rien relire après elle.
    pub spaces: Vec<(Space, Role)>,
    /// Le compte de la session remplacée (`replaces`), supprimée dans la transaction : à
    /// l'appelant de fermer ses connexions. `None` : aucune session donnée, ou un jeton
    /// inconnu.
    pub replaced: Option<Uuid>,
}

impl Store {
    /// Inscrit un compte, en une transaction (`BEGIN IMMEDIATE`) :
    /// 1. crée le compte ; le premier de l'instance en est l'admin ;
    /// 2. s'il est le premier, le fait `owner` de tout espace sans membre ;
    /// 3. avec un jeton d'invitation (`invitation`, son SHA-256) : l'accepte si elle vaut
    ///    encore à `now`, comme [`Store::accept_invitation`]. Sinon le compte reste sans
    ///    espace (`invitation_withdrawn`) : il venait pour elle, pas pour un espace à lui ;
    /// 4. sans jeton, s'il n'a aucun espace : crée `first_space_name`, dont il est `owner` ;
    /// 5. ouvre la session du compte (`session`) et supprime celle qu'elle remplace
    ///    (`replaces`, la session que portait déjà le navigateur, échue ou non).
    ///
    /// Tout est écrit, ou rien : sur une erreur (`EmailTaken` si l'adresse est prise), aucun
    /// compte n'existe et l'adresse reste libre. La session en est (report m1 du lot 2) : ouverte
    /// après la transaction, une panne entre les deux répondait par une erreur pour un compte
    /// pourtant créé, sans session, adresse prise et invitation consommée ; qui recommençait se
    /// voyait répondre que l'adresse était prise. La session remplacée aussi, pour la même raison
    /// (Ruling R265) : rien de ce qui suit le commit ne peut plus échouer.
    ///
    /// Le store ne juge pas du droit de s'inscrire (inscriptions ouvertes ou non, jeton exigé
    /// ou non) : c'est l'affaire de l'appelant, avant.
    pub async fn sign_up(
        &self,
        user: NewUser,
        invitation: Option<[u8; 32]>,
        first_space_name: &str,
        session: NewSession,
        replaces: Option<[u8; 32]>,
        now: i64,
    ) -> Result<SignedUp, StoreError> {
        let mut tx = self.write().await?;
        let user = insert_user(&mut tx, user).await?;
        let adopted = if user.instance_admin {
            adopt_orphans(&mut tx, user.id).await?
        } else {
            Vec::new()
        };
        let (mut accepted, mut invitation_withdrawn, mut space) = (None, false, None);
        match invitation {
            Some(token_hash) => match acceptable(&mut tx, token_hash, now).await? {
                Some(invitation) => {
                    // Un compte neuf n'a aucun rôle : l'invitation lui donne toujours quelque
                    // chose, et elle est consommée (R257 ne joue pas ici).
                    let gave = accept(&mut tx, &invitation, user.id, now).await?;
                    debug_assert!(gave, "une invitation donne toujours à un compte neuf");
                    accepted = Some(invitation);
                }
                None => invitation_withdrawn = true,
            },
            None if adopted.is_empty() => {
                space = Some(own_space(&mut tx, first_space_name, user.id).await?);
            }
            None => {}
        }
        insert_session(&mut tx, user.id, session).await?;
        let replaced = match replaces {
            Some(token_hash) => remove_session(&mut tx, token_hash).await?,
            None => None,
        };
        let spaces = spaces_of_in(&mut tx, user.id).await?;
        tx.commit().await?;
        Ok(SignedUp {
            user,
            adopted,
            accepted,
            invitation_withdrawn,
            space,
            spaces,
            replaced,
        })
    }
}

/// Crée, dans la transaction, un espace dont `owner` est le seul membre, au rôle `owner` ; sa
/// révision passe à 1 (une écriture dans `members`), comme avec [`Store::create_space`].
async fn own_space(tx: &mut WriteTx, name: &str, owner: Uuid) -> Result<Space, StoreError> {
    let mut space = insert_space(tx, name, Some(owner)).await?;
    let id = space.id.to_string();
    raise_role(tx, &id, &owner.to_string(), Role::Owner).await?;
    bump_rev(tx, &id).await?;
    space.rev += 1;
    Ok(space)
}
