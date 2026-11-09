//! Droits de Mementum : niveaux, rôles et calcul des accès d'un espace.
//!
//! - Les types écrits en base et dans l'API ([`Level`], [`Role`], [`Section`], [`Subject`]) ont
//!   une seule forme texte : `as_str` l'écrit (`Display` pour [`Subject`], qui porte un uuid),
//!   `parse` ne relit qu'elle.
//! - `access.rs` : [`SpaceAccess::compute`], le calcul pur de ce qu'une personne voit d'un
//!   espace, et à quel niveau.
//! - `cache.rs` : [`AccessCache`], ces calculs gardés tant que la révision de l'espace ne bouge
//!   pas.
//!
//! Les règles sont celles du « Modèle des droits » du plan du lot 2
//! (`docs/superpowers/plans/2026-11-10-p1-lot2-comptes-espaces-droits.md`).

mod access;
mod cache;

use std::fmt;

use uuid::Uuid;

pub use access::{NodeAccess, SpaceAccess, TreeNode};
pub use cache::AccessCache;

/// Niveau d'accès à une page, du plus faible au plus fort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Peut lire.
    Read,
    /// Peut commenter. Au lot 2, les mêmes droits que `Read` : aucune route ne commente encore.
    Comment,
    /// Peut modifier : contenu, titre, sous-pages, place parmi les mêmes frères, corbeille.
    Edit,
    /// Accès complet : changer de parent, voir et changer les accès, inviter, supprimer.
    Full,
}

impl Level {
    /// `read`, `comment`, `edit` ou `full`.
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Read => "read",
            Level::Comment => "comment",
            Level::Edit => "edit",
            Level::Full => "full",
        }
    }

    /// Relit la forme écrite par [`Level::as_str`], et elle seule.
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "read" => Some(Level::Read),
            "comment" => Some(Level::Comment),
            "edit" => Some(Level::Edit),
            "full" => Some(Level::Full),
            _ => None,
        }
    }

    /// Ce niveau suffit-il pour `action` ?
    pub fn allows(self, action: Action) -> bool {
        self >= action.min_level()
    }
}

/// Rôle d'une personne dans un espace (`members.role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Owner,
    Admin,
    Member,
    /// Invité : ne voit que les pages où une ligne `user:<lui>` est posée (ou héritée).
    Guest,
}

impl Role {
    /// `owner`, `admin`, `member` ou `guest`.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Guest => "guest",
        }
    }

    /// Relit la forme écrite par [`Role::as_str`], et elle seule.
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "owner" => Some(Role::Owner),
            "admin" => Some(Role::Admin),
            "member" => Some(Role::Member),
            "guest" => Some(Role::Guest),
            _ => None,
        }
    }

    /// Membre de l'espace (`owner`, `admin` ou `member`) : une ligne `members` le vise.
    /// Faux pour un invité.
    pub fn is_member(self) -> bool {
        matches!(self, Role::Owner | Role::Admin | Role::Member)
    }
}

/// À qui une ligne d'accès donne son niveau. Forme texte (`Display`, [`Subject::parse`]) :
/// `user:<uuid>` (uuid en minuscules, avec tirets) ou `members`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject {
    /// Une personne.
    User(Uuid),
    /// Tous les membres de l'espace (rôles `owner`, `admin` et `member`), pas les invités.
    Members,
}

impl Subject {
    /// Relit la forme écrite par `Display`, et elle seule : une autre graphie du même uuid
    /// (majuscules, sans tirets, entre accolades, `urn:uuid:`) est refusée, pour qu'un sujet
    /// n'ait qu'une écriture en base.
    pub fn parse(s: &str) -> Option<Subject> {
        if s == "members" {
            return Some(Subject::Members);
        }
        let text = s.strip_prefix("user:")?;
        let id = Uuid::try_parse(text).ok()?;
        let mut buf = Uuid::encode_buffer();
        let canonical: &str = id.hyphenated().encode_lower(&mut buf);
        (canonical == text).then_some(Subject::User(id))
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Subject::User(id) => write!(f, "user:{}", id.hyphenated()),
            Subject::Members => f.write_str("members"),
        }
    }
}

/// Une ligne d'accès posée sur une page (`acl`) : `level` pour `subject`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub subject: Subject,
    pub level: Level,
}

/// Section de la barre latérale où une page apparaît pour une personne. Calculée, jamais
/// stockée : voir [`SpaceAccess::compute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// « Espace » : les pages des membres.
    Space,
    /// « Partagé ».
    Shared,
    /// « Privé » : les pages où personne d'autre n'a de ligne.
    Private,
}

impl Section {
    /// `space`, `shared` ou `private`.
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Space => "space",
            Section::Shared => "shared",
            Section::Private => "private",
        }
    }

    /// Relit la forme écrite par [`Section::as_str`], et elle seule.
    pub fn parse(s: &str) -> Option<Section> {
        match s {
            "space" => Some(Section::Space),
            "shared" => Some(Section::Shared),
            "private" => Some(Section::Private),
            _ => None,
        }
    }
}

/// Ce qu'on demande à faire sur une page. Le niveau minimal de chacune est dans
/// [`Level::allows`] ; la corbeille, elle, est vérifiée par l'appelant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Lire la page, son MMD, se synchroniser en lecture.
    Read,
    /// Commenter (lot 3).
    Comment,
    /// Écrire le contenu (`PUT` mmd, synchro en écriture).
    Edit,
    Rename,
    /// Créer une sous-page.
    CreateChild,
    /// Changer de place parmi les mêmes frères.
    Reorder,
    /// Mettre à la corbeille.
    Trash,
    /// Restaurer de la corbeille.
    Restore,
    /// Changer de parent, ou passer à la racine.
    Move,
    /// Voir et changer les accès, inviter sur la page.
    Share,
    /// Supprimer définitivement.
    Delete,
}

impl Action {
    /// Le niveau minimal de l'action (tableau « Action / Niveau minimal » du plan).
    fn min_level(self) -> Level {
        match self {
            Action::Read => Level::Read,
            Action::Comment => Level::Comment,
            Action::Edit
            | Action::Rename
            | Action::CreateChild
            | Action::Reorder
            | Action::Trash
            | Action::Restore => Level::Edit,
            Action::Move | Action::Share | Action::Delete => Level::Full,
        }
    }
}
