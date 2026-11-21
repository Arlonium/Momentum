//! Ce qu'un jeton d'API vivant retire aux droits de son compte (lot 5, Rulings R1216 à R1219).
//!
//! - **Portée** ([`Scope`]) : tous les espaces du compte, un seul, ou des pages d'un seul espace
//!   avec leurs sous-pages, lues par les parents visibles (Ruling R1574) ;
//! - **niveau** : `read` ou `edit` (pas de `full` au lot 5, Ruling R1218), le plafond de tout
//!   niveau du compte ;
//! - **interdit** ([`TokenGrant::barred`]) : `Share`, `Delete` et `Move` pour tout jeton, `Trash`
//!   sans l'option `can_trash`, quel que soit le niveau.
//!
//! La restriction elle-même est dans `mementum-acl` (`SpaceAccess::narrowed`, `narrowed_node`) :
//! elle part du calcul du compte, ne lit aucune ligne d'accès, et ne peut que retirer.

use std::sync::Arc;

use mementum_acl::{Action, Barred, Level, NodeAccess, SpaceAccess};
use mementum_store::{LiveToken, TokenLevel, TokenScope};
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

    /// Les actions refusées à ce jeton quel que soit le niveau.
    pub fn barred(&self) -> Barred {
        if self.can_trash {
            ALWAYS_BARRED
        } else {
            ALWAYS_BARRED.union(Barred::of(&[Action::Trash]))
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
        NodeAccess {
            level: access.level.min(self.level),
            section: match self.scope {
                Scope::Pages { .. } => mementum_acl::Section::Shared,
                Scope::All | Scope::Space(_) => access.section,
            },
            barred: access.barred.union(self.barred()),
            ..access
        }
    }
}
