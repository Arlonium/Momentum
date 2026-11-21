//! Ce qu'un jeton d'API vivant retire aux droits de son compte (lot 5, Rulings R1216 à R1219).
//!
//! - **Portée** ([`Scope`]) : tous les espaces du compte, un seul, ou des pages d'un seul espace
//!   avec leurs sous-pages, lues par les parents visibles (Ruling R1574) ;
//! - **niveau** : `read` ou `edit` (pas de `full` au lot 5, Ruling R1218), le plafond de tout
//!   niveau du compte ;
//! - **interdit** ([`TokenGrant::barred`]) : `Share`, `Delete` et `Move` pour tout jeton, `Trash`
//!   et `Restore` sans l'option `can_trash`, quel que soit le niveau.
//!
//! La restriction elle-même est dans `mementum-acl` (`SpaceAccess::narrowed`, `narrowed_node`) :
//! elle part du calcul du compte, ne lit aucune ligne d'accès, et ne peut que retirer.

use std::sync::Arc;

use mementum_acl::{Action, Barred, Level, NodeAccess, SpaceAccess};
use mementum_store::{LiveToken, TokenLevel, TokenScope};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Par où un appel est venu : une session (cookie), ou un jeton d'API (`Authorization: Bearer
/// mmt_…`, Ruling R1214).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    Session,
    Token(Arc<TokenGrant>),
}

/// Ce qu'un jeton ouvre (Ruling R1219).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Tous les espaces du compte.
    All,
    /// Un seul espace.
    Space(Uuid),
    /// Des pages d'un seul espace, avec leurs sous-pages ; triées par identifiant. Aucune : le
    /// jeton n'ouvre plus rien (ses pages sont toutes parties).
    Pages { space: Uuid, roots: Vec<Uuid> },
}

/// Un jeton vivant, tel que les droits le lisent : jamais son texte ni son hachage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenGrant {
    pub token_id: Uuid,
    /// Le plus haut niveau qu'un appel par ce jeton peut avoir sur une page.
    pub level: Level,
    pub scope: Scope,
    /// La personne a permis la corbeille à ce jeton (Ruling R1218).
    pub can_trash: bool,
}

/// Ce que tout jeton se voit refuser, quel que soit son niveau (Ruling R1218) : partager,
/// supprimer pour de bon, et changer une page de parent ou la passer à la racine (ce qui peut
/// changer qui la voit).
const ALWAYS_BARRED: Barred = Barred::of(&[Action::Share, Action::Delete, Action::Move]);

impl TokenGrant {
    /// Le jeton relu par le store.
    pub fn from_live(token: &LiveToken) -> TokenGrant {
        let scope = match &token.scope {
            TokenScope::All => Scope::All,
            TokenScope::Space(space) => Scope::Space(*space),
            TokenScope::Pages { space, pages } => {
                let mut roots = pages.clone();
                roots.sort_unstable();
                roots.dedup();
                Scope::Pages {
                    space: *space,
                    roots,
                }
            }
        };
        TokenGrant {
            token_id: token.id,
            level: match token.level {
                TokenLevel::Read => Level::Read,
                TokenLevel::Edit => Level::Edit,
            },
            scope,
            can_trash: token.can_trash,
        }
    }

    /// Les actions refusées à ce jeton quel que soit le niveau. Sans l'option corbeille, ni mettre à
    /// la corbeille, ni en sortir (Ruling R1345 récrit : deux gestes sous confirmation humaine,
    /// §6.5, que l'option coche à la place de la personne).
    pub fn barred(&self) -> Barred {
        if self.can_trash {
            ALWAYS_BARRED
        } else {
            ALWAYS_BARRED.union(Barred::of(&[Action::Trash, Action::Restore]))
        }
    }

    /// La portée du jeton touche-t-elle l'espace `space` ? Jugé avant de lire quoi que ce soit
    /// de l'espace (la règle de R99).
    pub fn opens(&self, space: Uuid) -> bool {
        match &self.scope {
            Scope::All => true,
            Scope::Space(only) | Scope::Pages { space: only, .. } => *only == space,
        }
    }

    /// Les racines d'une portée de pages, triées ; `None` pour tout un espace.
    pub fn roots(&self) -> Option<&[Uuid]> {
        match &self.scope {
            Scope::Pages { roots, .. } => Some(roots),
            Scope::All | Scope::Space(_) => None,
        }
    }

    /// Ce que le compte voit de l'espace, restreint au jeton : pour les routes de tout
    /// l'espace (l'arbre, l'ensemble des pages permises de la recherche).
    pub fn narrow_space(&self, account: &SpaceAccess) -> SpaceAccess {
        account.narrowed(self.level, self.barred(), self.roots())
    }

    /// Les accès du compte à la seule page `id`, restreints au jeton, sans restreindre tout
    /// l'espace (Ruling R1256) ; `None` hors de la portée.
    pub fn narrow_node(&self, account: &SpaceAccess, id: Uuid) -> Option<NodeAccess> {
        account.narrowed_node(id, self.level, self.barred(), self.roots())
    }

    /// Les accès que le compte aurait sur une page qu'il vient de créer ou de placer, restreints
    /// au jeton (`AppState::own_access`, Ruling R1216) : la page est dans sa portée (elle est sous
    /// une page qu'il voit, ou à la racine d'un espace qu'il ouvre).
    pub fn narrow_own(&self, access: NodeAccess) -> NodeAccess {
        access.narrowed(self.level, self.barred(), self.roots().is_some())
    }

    /// La forme du jeton, pour le cache des restrictions (Ruling R1354) : ce qui décide de ce qu'il
    /// voit d'un espace, et rien d'autre (le compte, le niveau, l'interdit, les racines triées ; une
    /// portée « tous les espaces » et une portée « cet espace » restreignent cet espace pareil).
    /// Deux jetons de même forme partagent l'entrée ; un hachage préfixé, qu'aucun compte ne porte.
    pub fn shape(&self, account: Uuid) -> [u8; 16] {
        let mut hash = Sha256::new()
            .chain_update(b"mementum:forme-de-jeton:")
            .chain_update(account.as_bytes())
            .chain_update(self.level.as_str().as_bytes())
            .chain_update([u8::from(self.can_trash)]);
        match self.roots() {
            None => hash.update([0]),
            Some(roots) => {
                hash.update([1]);
                hash.update((roots.len() as u64).to_le_bytes());
                for root in roots {
                    hash.update(root.as_bytes());
                }
            }
        }
        let digest = hash.finalize();
        let mut shape = [0u8; 16];
        shape.copy_from_slice(&digest[..16]);
        shape
    }
}

#[cfg(test)]
mod tests {
    use mementum_acl::Section;

    use super::*;

    fn grant(level: Level, scope: Scope, can_trash: bool) -> TokenGrant {
        TokenGrant {
            token_id: Uuid::nil(),
            level,
            scope,
            can_trash,
        }
    }

    /// Lot 5, correction groupée (mutation S5-09 de la relecture finale) : les accès d'une page
    /// qu'un jeton vient de créer ou de placer (`AppState::own_access`) portent l'interdit du
    /// jeton, et le niveau ramené sous le sien ; « Partagé » pour un jeton à racines.
    #[test]
    fn an_own_page_carries_the_bar_and_the_level_of_the_token() {
        let full = NodeAccess {
            level: Level::Full,
            section: Section::Private,
            in_trash: false,
            visible_parent: None,
            barred: Barred::default(),
            capped_from: None,
        };
        let always = [Action::Share, Action::Delete, Action::Move];
        let edit = grant(Level::Edit, Scope::All, false).narrow_own(full);
        assert_eq!(edit.level, Level::Edit);
        assert_eq!(edit.section, Section::Private);
        for action in always.into_iter().chain([Action::Trash, Action::Restore]) {
            assert!(!edit.allows(action), "{action:?}");
        }
        assert!(edit.allows(Action::Edit) && edit.allows(Action::Rename));
        let trashing = grant(Level::Edit, Scope::All, true).narrow_own(full);
        assert!(trashing.allows(Action::Trash) && trashing.allows(Action::Restore));
        for action in always {
            assert!(!trashing.allows(action), "{action:?}");
        }
        let pages = Scope::Pages {
            space: Uuid::nil(),
            roots: vec![Uuid::nil()],
        };
        let read = grant(Level::Read, pages, false).narrow_own(full);
        assert_eq!((read.level, read.section), (Level::Read, Section::Shared));
        assert!(!read.allows(Action::Edit));
    }
}
