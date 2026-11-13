//! Types de l'API REST, partagés avec le client web.
//!
//! `ts-rs` les traduit en TypeScript dans `packages/protocol/src/generated/`
//! (fichiers commités). Après une modification ici :
//! `UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed`.
//!
//! Les identifiants sont des UUID écrits en texte. Les horodatages sont en
//! millisecondes Unix, des entiers sûrs en JavaScript (< 2⁵³) : ils sont
//! déclarés `number` et non `bigint`, puisque `JSON.parse` rend un nombre.

use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

/// Une page, telle que la voit le compte de la session : dans l'arbre d'un espace, dans sa
/// corbeille, et dans la réponse de chaque route de page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PageNode {
    pub id: String,
    /// L'espace de la page.
    pub space_id: String,
    /// La page parente, si le compte la voit. `null` : la page est à la racine de l'espace,
    /// ou sous une page qu'il ne voit pas ; elle est alors à la racine de ce qu'il voit.
    pub parent_id: Option<String>,
    /// `"page"` (aucune route ne crée encore autre chose).
    pub kind: String,
    pub title: String,
    /// Index fractionnaire : l'ordre des frères est l'ordre de ces chaînes,
    /// comparées unité par unité (ASCII).
    pub position: String,
    /// Le niveau du compte sur la page : le plus fort de ceux que lui donnent la page et ses
    /// ancêtres.
    pub level: Level,
    /// La section de la barre latérale où la page apparaît pour le compte.
    pub section: Section,
    /// Millisecondes Unix.
    #[ts(type = "number")]
    pub updated_at: i64,
    /// Millisecondes Unix : depuis quand la page est à la corbeille, qu'elle y ait été mise
    /// elle-même ou qu'elle y soit par un ancêtre (c'est alors la date du plus proche).
    /// `null` : elle n'y est pas. Une page à la corbeille se lit, et ne s'écrit plus.
    #[ts(type = "number | null")]
    pub trashed_at: Option<i64>,
}

/// Les pages d'un espace que le compte de la session voit, hors corbeille
/// (`GET /api/v1/spaces/{id}/tree`), triées par `(parentId, position)`, la racine d'abord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub space_id: String,
    /// La version de l'espace (`spaces.lsn`), lue avant les droits et les pages : l'arbre est
    /// au moins aussi récent qu'elle (Ruling R141). Le client la compare aux trames du canal de
    /// l'espace (`/events/{spaceId}`) : au pire il relit une fois de trop, jamais il ne manque
    /// un changement.
    #[ts(type = "number")]
    pub lsn: i64,
    pub nodes: Vec<PageNode>,
}

/// Les pages favorites du compte de la session dans un espace
/// (`GET /api/v1/spaces/{id}/favorites`, Ruling R254).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Favorites {
    /// Les identifiants des pages, dans l'ordre où le compte les a ajoutées : seulement celles
    /// qu'il voit, hors corbeille. Une page qu'il ne voit plus, ou à la corbeille, n'y est pas ;
    /// elle y revient à sa place avec l'accès, ou restaurée. Titre et niveau se lisent dans
    /// l'arbre de l'espace ([`Tree`]), qui montre les mêmes pages.
    pub pages: Vec<String>,
}

/// Version du serveur et de son schéma de blocs (`GET /api/v1/info`). Un
/// client d'un autre schéma passe en lecture seule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub version: String,
    pub schema_version: u32,
    /// Qui peut créer un compte.
    pub signup: SignupMode,
    /// Aucun compte n'existe encore : le premier compte créé administrera
    /// l'instance, quel que soit `signup`.
    pub setup: bool,
}

/// Qui peut créer un compte (`MEMENTUM_SIGNUP`). Le premier compte de
/// l'instance est permis dans les deux cas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SignupMode {
    /// Tout le monde.
    Open,
    /// Qui porte le jeton d'une invitation (Ruling R50).
    #[default]
    Invite,
}

impl SignupMode {
    /// `open` ou `invite`.
    pub fn as_str(self) -> &'static str {
        match self {
            SignupMode::Open => "open",
            SignupMode::Invite => "invite",
        }
    }

    /// Relit la forme écrite par [`SignupMode::as_str`], et elle seule.
    pub fn parse(s: &str) -> Option<SignupMode> {
        match s {
            "open" => Some(SignupMode::Open),
            "invite" => Some(SignupMode::Invite),
            _ => None,
        }
    }
}

impl FromStr for SignupMode {
    type Err = String;

    /// Pour l'option `--signup` : l'erreur est le message montré.
    fn from_str(s: &str) -> Result<SignupMode, String> {
        SignupMode::parse(s).ok_or_else(|| "attendu open ou invite".to_string())
    }
}

/// Corps de `POST /api/v1/auth/signup`.
// Sans `Debug`, comme tout type qui porte un mot de passe (`LogIn`,
// `ChangePassword`) : il ne peut pas finir dans un journal par mégarde.
#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignUp {
    /// Rognée et mise en minuscules ; jamais vérifiée (pas de SMTP, Ruling
    /// R47) : elle ne donne aucun droit.
    pub email: String,
    /// Nom affiché : rogné, 1 à 100 caractères.
    pub name: String,
    /// 10 à 200 caractères.
    pub password: String,
    /// Jeton d'une invitation, acceptée dans la foulée. Exigé quand les
    /// inscriptions sont sur invitation et qu'un compte existe déjà.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub invitation: Option<String>,
}

/// Corps de `POST /api/v1/auth/login`.
#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogIn {
    pub email: String,
    pub password: String,
}

/// Corps de `POST /api/v1/auth/password`.
#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangePassword {
    /// Le mot de passe actuel.
    pub current: String,
    /// Le nouveau : 10 à 200 caractères.
    pub new: String,
}

/// Le compte de la session et ses espaces (`GET /api/v1/me`, réponses de
/// l'inscription et de la connexion).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub user: UserInfo,
    /// Les espaces où le compte a un rôle (invité compris), du plus ancien
    /// au plus récent.
    pub spaces: Vec<SpaceInfo>,
}

/// Un compte, tel que son titulaire le voit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub name: String,
    /// Admin de l'instance : le premier compte créé.
    pub instance_admin: bool,
}

/// Un espace, et le rôle qu'y a le compte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInfo {
    pub id: String,
    pub name: String,
    pub role: Role,
}

/// Rôle d'une personne dans un espace.
// La forme d'API de `mementum_acl::Role`, écrite comme `Role::as_str`
// (test `role_is_written_like_the_acl_role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Member,
    /// Invité : ne voit que les pages partagées avec lui.
    Guest,
}

impl From<mementum_acl::Role> for Role {
    fn from(role: mementum_acl::Role) -> Role {
        match role {
            mementum_acl::Role::Owner => Role::Owner,
            mementum_acl::Role::Admin => Role::Admin,
            mementum_acl::Role::Member => Role::Member,
            mementum_acl::Role::Guest => Role::Guest,
        }
    }
}

impl From<Role> for mementum_acl::Role {
    fn from(role: Role) -> mementum_acl::Role {
        match role {
            Role::Owner => mementum_acl::Role::Owner,
            Role::Admin => mementum_acl::Role::Admin,
            Role::Member => mementum_acl::Role::Member,
            Role::Guest => mementum_acl::Role::Guest,
        }
    }
}

/// Niveau d'accès à une page, du plus faible au plus fort.
// La forme d'API de `mementum_acl::Level`, écrite comme `Level::as_str` (test
// `level_is_written_like_the_acl_level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Peut lire.
    Read,
    /// Peut commenter : lire, ouvrir un fil, répondre, réagir (Ruling R134).
    Comment,
    /// Peut modifier.
    Edit,
    /// Accès complet : déplacer, partager, supprimer.
    Full,
}

impl From<mementum_acl::Level> for Level {
    fn from(level: mementum_acl::Level) -> Level {
        match level {
            mementum_acl::Level::Read => Level::Read,
            mementum_acl::Level::Comment => Level::Comment,
            mementum_acl::Level::Edit => Level::Edit,
            mementum_acl::Level::Full => Level::Full,
        }
    }
}

impl From<Level> for mementum_acl::Level {
    fn from(level: Level) -> mementum_acl::Level {
        match level {
            Level::Read => mementum_acl::Level::Read,
            Level::Comment => mementum_acl::Level::Comment,
            Level::Edit => mementum_acl::Level::Edit,
            Level::Full => mementum_acl::Level::Full,
        }
    }
}

/// Section de la barre latérale où une page apparaît pour une personne. Calculée pour chacun,
/// jamais stockée : les sous-pages suivent celle de la plus haute page que la personne voit.
// La forme d'API de `mementum_acl::Section`, écrite comme `Section::as_str` (test
// `section_is_written_like_the_acl_section`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    /// « Espace » : les pages des membres.
    Space,
    /// « Partagé » : les pages où quelqu'un d'autre est nommé. Un invité ne voit qu'elle.
    Shared,
    /// « Privé » : les pages où personne d'autre n'a d'accès.
    Private,
}

impl From<mementum_acl::Section> for Section {
    fn from(section: mementum_acl::Section) -> Section {
        match section {
            mementum_acl::Section::Space => Section::Space,
            mementum_acl::Section::Shared => Section::Shared,
            mementum_acl::Section::Private => Section::Private,
        }
    }
}

/// La section où créer une page à la racine d'un espace. « Partagé » ne se choisit pas : une
/// page y arrive quand on la partage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RootSection {
    /// « Espace » : la page reçoit l'accès `(members, full)`.
    Space,
    /// « Privé » : la page reçoit l'accès `(user:<auteur>, full)`.
    #[default]
    Private,
}

/// Corps de `POST /api/v1/spaces`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSpace {
    /// Rogné : 1 à 100 caractères.
    pub name: String,
}

/// Corps de `PATCH /api/v1/spaces/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSpace {
    /// Rogné : 1 à 100 caractères.
    pub name: String,
}

/// Une personne qui a un rôle dans un espace (`GET /api/v1/spaces/{id}/members`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub role: Role,
}

/// Corps de `PATCH /api/v1/spaces/{id}/members/{userId}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMember {
    pub role: Role,
}

/// Le rôle que donne une invitation dans un espace : on n'invite ni un propriétaire (il se
/// nomme parmi les membres) ni un invité (il vient d'une invitation sur une page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum InviteRole {
    Admin,
    Member,
}

impl From<InviteRole> for mementum_acl::Role {
    fn from(role: InviteRole) -> mementum_acl::Role {
        match role {
            InviteRole::Admin => mementum_acl::Role::Admin,
            InviteRole::Member => mementum_acl::Role::Member,
        }
    }
}

/// Corps de `POST /api/v1/spaces/{id}/invitations`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InviteToSpace {
    /// Rognée et mise en minuscules ; gardée pour l'affichage, jamais vérifiée : c'est le
    /// jeton qui fait foi (Ruling R47).
    pub email: String,
    pub role: InviteRole,
}

/// Une invitation créée. Le jeton n'est donné qu'ici, une seule fois : le serveur n'en garde
/// que le SHA-256.
// Sans `Debug`, comme tout type qui porte un secret : il ne peut pas finir dans un journal par
// mégarde.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InvitationCreated {
    pub id: String,
    /// 32 octets aléatoires en base64url, sans remplissage : à mettre dans le lien
    /// d'invitation.
    pub token: String,
}

/// Une invitation qui attend d'être acceptée, sans son jeton
/// (`GET /api/v1/spaces/{id}/invitations`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvitation {
    pub id: String,
    pub email: String,
    /// Le rôle donné dans l'espace ; `null` pour une invitation sur une page.
    pub role: Option<Role>,
    /// Le niveau donné sur la page ; `null` pour une invitation dans l'espace.
    pub level: Option<Level>,
    /// La page ; `null` pour une invitation dans l'espace.
    pub page_id: Option<String>,
    /// Le titre de la page, tel qu'il est à la lecture (vide pour une page sans titre) ;
    /// `null` pour une invitation dans l'espace. Il ne sort que vers qui voit l'invitation :
    /// qui peut partager la page.
    pub page_title: Option<String>,
    /// Millisecondes Unix : l'invitation vaut jusqu'à cet instant, exclu.
    #[ts(type = "number")]
    pub expires_at: i64,
}

/// Corps de `POST /api/v1/invitations/lookup` et `POST /api/v1/invitations/accept`.
// Sans `Debug` : il porte le jeton.
#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvitationToken {
    pub token: String,
}

/// Ce que dit un lien d'invitation, avant de l'accepter (`POST /api/v1/invitations/lookup`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InvitationInfo {
    pub space_name: String,
    /// Le titre de la page ; `null` pour une invitation dans l'espace.
    pub page_title: Option<String>,
    /// Le nom de qui a invité.
    pub invited_by: String,
    /// L'adresse à qui l'invitation a été faite (pour l'affichage : c'est le jeton qui fait
    /// foi).
    pub email: String,
}

/// Qui a accès à une page, et à quel niveau (`GET /api/v1/pages/{id}/access`, et la réponse des
/// routes qui changent ces accès). Rendu seulement à qui a `full` sur la page : il porte le nom
/// et l'adresse des comptes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PageAccess {
    /// L'accès général effectif : le niveau que la ligne `members` de la page, ou d'un ancêtre
    /// (la plus forte), donne aux membres de l'espace (les propriétaires et les admins y ont
    /// `full`). `null` : seulement les personnes nommées ci-dessous.
    pub general: Option<Level>,
    /// Le plancher de l'accès général (Ruling R93) : le niveau que la ligne `members` d'un
    /// ancêtre donne aux membres (la plus forte des ancêtres). On ne le baisse ni ne le retire
    /// ici (`409 inherited_access`) : les choix plus bas sont à griser. `null` : aucun ancêtre
    /// n'en a.
    pub inherited_general: Option<Level>,
    /// Les comptes qui ont un rôle dans l'espace et une ligne sur la page ou sur un ancêtre,
    /// une fois chacun, dans l'ordre de leur arrivée dans l'espace.
    pub entries: Vec<AccessEntry>,
    /// Les invitations sur cette page qui attendent encore, sans leur jeton.
    pub invitations: Vec<PendingInvitation>,
}

/// Un compte nommé dans les accès d'une page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccessEntry {
    pub user_id: String,
    pub name: String,
    pub email: String,
    /// Le niveau que lui donnent ses lignes nommées : le plus fort de celles de la page et de
    /// ses ancêtres. L'accès général (`general`) s'y ajoute pour un membre ; un propriétaire ou
    /// un admin a même `full` là où il s'applique, ce que ce niveau ne dit pas.
    pub level: Level,
    /// Ce niveau vient d'un ancêtre : il se change sur la page qui le porte. Un compte qui a
    /// une ligne sur la page et un niveau hérité plus fort apparaît ainsi, une fois.
    pub inherited: bool,
}

/// Corps de `PUT /api/v1/pages/{id}/access/general`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetGeneral {
    /// Le niveau de la ligne `members` de la page ; `null` : la ligne est retirée (on revient au
    /// plancher d'une page parente, `inheritedGeneral`, ou sans lui à « seulement les personnes
    /// nommées »). Au plancher, la ligne part aussi ; dessous, `409 inherited_access` (Ruling
    /// R93). Obligatoire : un corps sans lui ne retire rien.
    #[serde(deserialize_with = "nullable")]
    pub level: Option<Level>,
}

/// Corps de `PUT /api/v1/pages/{id}/access/users/{userId}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetLevel {
    pub level: Level,
}

/// Corps de `POST /api/v1/pages/{id}/invitations`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InviteToPage {
    /// Rognée et mise en minuscules ; gardée pour l'affichage, jamais vérifiée : c'est le
    /// jeton qui fait foi (Ruling R47). La réponse ne dit pas si un compte la porte.
    pub email: String,
    /// Le niveau que donne l'invitation sur la page (et, par héritage, ses sous-pages).
    pub level: Level,
}

/// Une invitation acceptée (`POST /api/v1/invitations/accept`) : où aller ensuite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub space_id: String,
    /// La page ; `null` pour une invitation dans l'espace.
    pub page_id: Option<String>,
    /// L'invitation a donné quelque chose au compte, et ne sert plus. `false` (Ruling R257) : il
    /// avait déjà ce rôle ou plus, ou, pour une page, ce niveau ou plus (par une ligne à son
    /// nom, une page parente ou l'accès général) ; rien n'a changé, et le lien reste valable
    /// pour quelqu'un d'autre.
    pub changed: bool,
}

/// Corps de `POST /api/v1/pages`. Il faut dire où va la page : `parentId` (une sous-page), ou
/// `spaceId` (à la racine de cet espace).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePage {
    /// L'espace. Obligatoire à la racine ; avec `parentId`, facultatif, et c'est alors celui
    /// de la page parente.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub space_id: Option<String>,
    /// Absent ou `null` : à la racine de l'espace.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub parent_id: Option<String>,
    /// Absent : en fin des frères ; `null` : en tête ; un id : après ce frère.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional)]
    pub after_id: Option<Option<String>>,
    /// Absent, `null` ou blanc : « Sans titre ». Espaces de bord retirées,
    /// 500 caractères au plus.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub title: Option<String>,
    /// À la racine seulement : la section, donc les accès que reçoit la page. Absent ou
    /// `null` : `private`. Refusé avec `parentId` : une sous-page hérite de sa page parente.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub section: Option<RootSection>,
}

/// Corps de `PATCH /api/v1/pages/{id}`. Un champ absent ne change rien.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePage {
    /// Nouveau titre (mêmes règles qu'à la création) ; absent ou `null` : le
    /// titre ne change pas.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub title: Option<String>,
    /// Nouveau parent ; `null` : la racine. Sans `afterId`, la page va en fin
    /// des enfants du nouveau parent.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional)]
    pub parent_id: Option<Option<String>>,
    /// Frère après lequel placer la page ; `null` : en tête. Sans `parentId`,
    /// la page reste chez son parent actuel.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional)]
    pub after_id: Option<Option<String>>,
}

/// Un compte, tel que le nomme un fil de commentaires : son identifiant et son nom affiché,
/// jamais son adresse (un invité ne lit pas celle des autres). Un compte qui n'est plus dans
/// l'espace reste l'auteur de ses messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub id: String,
    /// Le nom montré pour le compte, comme dans la liste des présents de la page : sans
    /// caractère de contrôle, 100 caractères au plus, « Sans nom » s'il n'en reste rien.
    pub name: String,
}

/// Les comptes qui ont posé un même emoji sur un message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    /// De 1 à 16 octets, sans espace ni caractère de contrôle.
    pub emoji: String,
    /// Dans l'ordre où ils l'ont posé.
    pub users: Vec<Author>,
}

/// Un message d'un fil de commentaires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub author: Author,
    /// Le texte, sans espaces de bord : de 1 à 10 000 caractères, sauts de ligne compris.
    pub body: String,
    /// Millisecondes Unix.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Millisecondes Unix : la dernière modification du texte ; `null` s'il n'a jamais changé.
    #[ts(type = "number | null")]
    pub edited_at: Option<i64>,
    /// Dans l'ordre où chaque emoji est apparu sur le message.
    pub reactions: Vec<Reaction>,
}

/// Un fil de commentaires d'une page, avec ses messages (`GET /api/v1/pages/{id}/comments`,
/// Ruling R134). Rien n'en est écrit dans le document de la page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    /// L'identifiant du bloc auquel le fil est attaché ; `null` : un commentaire de la page.
    /// Il reste quand le bloc disparaît du document : le fil se lit toujours.
    pub block_id: Option<String>,
    /// Qui a ouvert le fil : il le reste quand son premier message est supprimé. C'est lui qui
    /// peut le résoudre ou le rouvrir avec « Peut commenter » (les autres : « Peut modifier »).
    pub opened_by: Author,
    /// Millisecondes Unix.
    #[ts(type = "number")]
    pub created_at: i64,
    /// Millisecondes Unix ; `null` : le fil est ouvert.
    #[ts(type = "number | null")]
    pub resolved_at: Option<i64>,
    pub resolved_by: Option<Author>,
    /// Du plus ancien au plus récent ; un message modifié garde sa place.
    pub comments: Vec<Comment>,
}

/// Corps de `POST /api/v1/pages/{id}/comments`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewThread {
    /// Le bloc où attacher le fil : il doit être dans le document de la page
    /// (`422 invalid_block` sinon). Absent ou `null` : un commentaire de la page.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub block_id: Option<String>,
    /// Le premier message : espaces de bord retirées, de 1 à 10 000 caractères, sans caractère
    /// de contrôle autre que le saut de ligne (`422 invalid_body` sinon).
    pub body: String,
}

/// Corps de `POST /api/v1/pages/{id}/comments/{threadId}/replies` et de
/// `PATCH /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewComment {
    /// Mêmes règles que le premier message d'un fil.
    pub body: String,
}

/// Corps de `PATCH /api/v1/pages/{id}/comments/{threadId}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveThread {
    /// `true` : résoudre le fil ; `false` : le rouvrir. Déjà dans cet état, rien ne change.
    pub resolved: bool,
}

/// Pourquoi une version d'une page a été gardée (Ruling R135).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum VersionReason {
    /// Avant l'écriture qui suivait de l'intervalle du serveur (dix minutes par défaut) la
    /// première écriture pas encore versionnée : l'état d'avant cette écriture.
    Periodic,
    /// Quand plus personne n'avait la page ouverte, ou que le serveur s'est arrêté.
    Closing,
    /// Juste avant une restauration : ce qu'elle a remplacé.
    BeforeRestore,
}

impl From<mementum_store::VersionReason> for VersionReason {
    fn from(reason: mementum_store::VersionReason) -> VersionReason {
        match reason {
            mementum_store::VersionReason::Periodic => VersionReason::Periodic,
            mementum_store::VersionReason::Closing => VersionReason::Closing,
            mementum_store::VersionReason::BeforeRestore => VersionReason::BeforeRestore,
        }
    }
}

/// Une version d'une page (`GET /api/v1/pages/{id}/versions`, Ruling R135) : l'état complet
/// de la page à un instant, que `GET …/versions/{id}/state` rend en octets Yjs. Une page garde
/// ses cent dernières.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    /// Millisecondes Unix.
    #[ts(type = "number")]
    pub created_at: i64,
    pub reason: VersionReason,
    /// Les comptes qui ont modifié la page depuis la version d'avant, dans l'ordre de leurs
    /// identifiants, nommés comme dans les commentaires ; vide si aucune écriture n'avait de
    /// compte. Un compte qui n'est plus dans l'espace reste nommé.
    pub authors: Vec<Author>,
}

/// Code d'une erreur de l'API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Page, espace ou route introuvable ; ou que la personne n'a pas le droit de voir : la
    /// réponse est la même.
    NotFound,
    /// Méthode non permise sur cette route.
    MethodNotAllowed,
    /// Déplacement d'une page sous elle-même ou sous une descendante.
    Cycle,
    /// MMD illisible (`line` donne la ligne).
    InvalidMmd,
    /// Blocs que le serveur n'écrit pas : structure ou profondeur que
    /// BlockNote refuse, prop hors registre écrite par MMD (Ruling R41).
    InvalidBlocks,
    /// `afterId` ne désigne pas un frère, que la personne voit, à l'endroit visé.
    InvalidAfter,
    /// `parentId` ne désigne pas une page que la personne voit dans l'espace.
    InvalidParent,
    /// Titre de plus de 500 caractères.
    TitleTooLong,
    /// Corps JSON illisible ou inattendu.
    InvalidJson,
    /// Requête mal formée : poignée de main WebSocket attendue sur `/sync`,
    /// ou corps de requête illisible.
    InvalidRequest,
    /// Corps de requête trop gros.
    PayloadTooLarge,
    /// La page ne répond pas pour l'instant ; réessayer.
    Unavailable,
    /// Erreur interne du serveur (détails dans ses journaux).
    Internal,
    /// En-tête `Host` qui ne nomme pas la boucle locale, alors que le serveur
    /// n'écoute que sur elle (DNS rebinding, Ruling R31). Statut 421.
    BadHost,
    /// En-tête `Origin` d'une autre page que celles de la boucle locale, du
    /// serveur ou de son adresse publique, à `/sync` et sur toute requête
    /// qui écrit (Rulings R31 et R51). Statut 403.
    BadOrigin,
    /// Pas de session : cookie absent, inconnu ou échu. Statut 401.
    Unauthenticated,
    /// Adresse inconnue, mot de passe faux ou compte sans mot de passe : la
    /// même réponse dans les trois cas. Statut 401.
    InvalidCredentials,
    /// Trop d'échecs de connexion pour cette adresse : réessayer après le
    /// délai de l'en-tête `Retry-After`. Statut 429.
    TooManyAttempts,
    /// Les inscriptions sont sur invitation. Statut 403.
    SignupClosed,
    /// Un compte porte déjà cette adresse e-mail. Statut 409.
    EmailTaken,
    /// Adresse e-mail mal formée. Statut 422.
    InvalidEmail,
    /// Nom vide ou trop long. Statut 422.
    InvalidName,
    /// Mot de passe trop court ou trop long. Statut 422.
    InvalidPassword,
    /// Jeton d'invitation inconnu, échu, révoqué ou déjà accepté : la même
    /// réponse dans les quatre cas. Statut 404.
    InvalidInvitation,
    /// Action au-dessus de son rôle dans un espace qu'on voit (ou de son niveau
    /// sur une page qu'on voit). Sans aucun rôle, c'est `not_found`. Statut 403.
    Forbidden,
    /// L'espace perdrait son dernier propriétaire. Statut 409.
    LastOwner,
    /// Écriture dans une page à la corbeille (elle-même, ou par un ancêtre) : la restaurer
    /// d'abord. Statut 409.
    Trashed,
    /// Restauration d'une page dont un ancêtre est à la corbeille : c'est lui qu'on restaure.
    /// Statut 409.
    ParentTrashed,
    /// Suppression définitive d'une page qui n'est pas à la corbeille. Statut 409.
    NotTrashed,
    /// Les rôles sur lesquels la demande était décidée ont changé pendant qu'elle
    /// s'écrivait, plusieurs fois de suite : rien n'est écrit ; réessayer. Statut 409.
    RolesChanged,
    /// La page a changé de place, ou ce que la personne peut en faire, à chaque tentative
    /// d'écrire son renommage ou son déplacement (Ruling R91) : rien n'est écrit ; réessayer.
    /// Statut 409.
    PageMoved,
    /// Le changement laisserait la page sans personne en accès complet, ni par ses accès ni
    /// par ceux d'une page parente : rien n'est écrit. Statut 409.
    LastFullAccess,
    /// L'accès général demandé sur une page (retiré, ou plus bas) est sous celui qu'une page
    /// parente donne déjà aux membres : il se change sur elle ; rien n'est écrit. Statut 409.
    InheritedAccess,
    /// Texte d'un message vide, de plus de 10 000 caractères une fois les espaces de bord
    /// retirées, ou qui porte un caractère de contrôle autre que le saut de ligne. Statut 422.
    InvalidBody,
    /// `blockId` ne nomme aucun bloc du document de la page. Statut 422.
    InvalidBlock,
    /// Emoji d'une réaction de plus de 16 octets, ou qui porte une espace ou un caractère de
    /// contrôle. Statut 422.
    InvalidEmoji,
    /// `PUT /mmd` de plus de 12 000 blocs, à tous les niveaux, lignes de tableau comprises
    /// (Ruling R259) : rien n'est écrit. Statut 422.
    TooManyBlocks,
    /// `PUT /mmd` dont l'écriture est estimée trop longue pour l'acteur de la page : une liste
    /// trop longue (frères, lignes ou cellules d'un tableau, morceaux du contenu en ligne d'un
    /// bloc) ou trop d'éléments (Ruling R368) ; le message dit lequel : rien n'est écrit. Statut
    /// 422.
    TooLongToWrite,
    /// La page porte déjà 2 000 fils de commentaires, résolus compris (Ruling R204), ou ce
    /// compte y en a déjà ouvert 200, sa part (Ruling R351) ; le message dit lequel : rien n'est
    /// écrit. Statut 409.
    ThreadLimit,
    /// Le fil porte déjà 500 messages (Ruling R204) : rien n'est écrit. Statut 409.
    MessageLimit,
    /// Le message porte déjà 32 emojis différents, et celui-ci n'en est pas (Ruling R204), ou ce
    /// compte y en a déjà posé 8, sa part (Ruling R351), ou la page porte déjà 20 000 réactions
    /// (Ruling R357), ou ce compte y en a déjà posé 2 000, sa part (Ruling R358) ; le message dit
    /// lequel : rien n'est écrit. Statut 409.
    ReactionLimit,
    /// Les commentaires de la page portent déjà 10 000 messages, ou l'écriture ferait passer
    /// leur texte au-delà de 4 Mio (Ruling R350), ou la part de ce compte au-delà de 2 000
    /// messages ou de 1 Mio de texte sur la page (Ruling R358) ; le message dit lequel : rien
    /// n'est écrit. Statut 409.
    PageCommentLimit,
}

/// Corps de toute réponse d'erreur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ErrorBody {
    pub error: ErrorCode,
    /// Explication en français.
    pub message: String,
    /// Ligne du MMD fautive (à partir de 1), pour `invalid_mmd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub line: Option<usize>,
}

/// Distingue un champ absent (`None`, par `#[serde(default)]`) d'un champ à
/// `null` (`Some(None)`).
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Un champ obligatoire qui peut valoir `null` : sans `#[serde(default)]`, il ne peut pas
/// manquer (serde ne le rend plus `None` d'office).
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Les fichiers TypeScript de `packages/protocol/src/generated/` : nom et
/// contenu, `index.ts` compris. Sert au test `bindings_are_committed`.
#[doc(hidden)]
pub fn bindings() -> Vec<(String, String)> {
    fn file<T: TS + 'static>(cfg: &ts_rs::Config) -> (String, String) {
        let content = T::export_to_string(cfg)
            .unwrap_or_else(|e| panic!("export TypeScript de {} : {e}", T::ident(cfg)));
        (format!("{}.ts", T::ident(cfg)), content)
    }
    // Réglages fixes : les variables `TS_RS_*` ne changent rien.
    let cfg = ts_rs::Config::new();
    let mut files = vec![
        file::<Accepted>(&cfg),
        file::<AccessEntry>(&cfg),
        file::<Author>(&cfg),
        file::<ChangePassword>(&cfg),
        file::<Comment>(&cfg),
        file::<CreatePage>(&cfg),
        file::<CreateSpace>(&cfg),
        file::<ErrorBody>(&cfg),
        file::<ErrorCode>(&cfg),
        file::<Favorites>(&cfg),
        file::<Info>(&cfg),
        file::<InvitationCreated>(&cfg),
        file::<InvitationInfo>(&cfg),
        file::<InvitationToken>(&cfg),
        file::<InviteRole>(&cfg),
        file::<InviteToPage>(&cfg),
        file::<InviteToSpace>(&cfg),
        file::<Level>(&cfg),
        file::<LogIn>(&cfg),
        file::<Me>(&cfg),
        file::<Member>(&cfg),
        file::<NewComment>(&cfg),
        file::<NewThread>(&cfg),
        file::<PageAccess>(&cfg),
        file::<PageNode>(&cfg),
        file::<PendingInvitation>(&cfg),
        file::<Reaction>(&cfg),
        file::<ResolveThread>(&cfg),
        file::<Role>(&cfg),
        file::<RootSection>(&cfg),
        file::<Section>(&cfg),
        file::<SetGeneral>(&cfg),
        file::<SetLevel>(&cfg),
        file::<SignUp>(&cfg),
        file::<SignupMode>(&cfg),
        file::<SpaceInfo>(&cfg),
        file::<Thread>(&cfg),
        file::<Tree>(&cfg),
        file::<UpdateMember>(&cfg),
        file::<UpdatePage>(&cfg),
        file::<UpdateSpace>(&cfg),
        file::<UserInfo>(&cfg),
        file::<Version>(&cfg),
        file::<VersionReason>(&cfg),
    ];
    let mut index = String::from(
        "// Généré par `mementum-server` (ts-rs) : ne pas modifier à la main.\n\
         // Régénérer : UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed\n\n",
    );
    for (name, _) in &files {
        let ty = name.trim_end_matches(".ts");
        index.push_str(&format!("export type {{ {ty} }} from \"./{ty}\";\n"));
    }
    files.push(("index.ts".to_string(), index));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le rôle de l'API s'écrit comme celui des droits, pour chacun des
    /// quatre.
    #[test]
    fn role_is_written_like_the_acl_role() {
        use mementum_acl::Role as Acl;
        for role in [Acl::Owner, Acl::Admin, Acl::Member, Acl::Guest] {
            let written = serde_json::to_value(Role::from(role)).unwrap();
            assert_eq!(written, role.as_str());
        }
    }

    /// Et dans l'autre sens, sans perte.
    #[test]
    fn role_roundtrips_to_the_acl_role() {
        use mementum_acl::Role as Acl;
        for role in [Acl::Owner, Acl::Admin, Acl::Member, Acl::Guest] {
            assert_eq!(Acl::from(Role::from(role)), role);
        }
        assert_eq!(Acl::from(InviteRole::Admin), Acl::Admin);
        assert_eq!(Acl::from(InviteRole::Member), Acl::Member);
    }

    /// Le niveau de l'API s'écrit comme celui des droits, pour chacun des
    /// quatre, et revient sans perte.
    #[test]
    fn level_is_written_like_the_acl_level() {
        use mementum_acl::Level as Acl;
        for level in [Acl::Read, Acl::Comment, Acl::Edit, Acl::Full] {
            let written = serde_json::to_value(Level::from(level)).unwrap();
            assert_eq!(written, level.as_str());
            assert_eq!(Acl::from(Level::from(level)), level);
        }
    }

    /// `SetGeneral.level` se donne toujours : `null` retire l'accès général, un corps sans
    /// lui est refusé (il ne retire rien).
    #[test]
    fn set_general_level_is_required_but_nullable() {
        let read: SetGeneral =
            serde_json::from_value(serde_json::json!({ "level": "read" })).unwrap();
        assert_eq!(read.level, Some(Level::Read));
        let none: SetGeneral =
            serde_json::from_value(serde_json::json!({ "level": null })).unwrap();
        assert_eq!(none.level, None);
        assert!(serde_json::from_value::<SetGeneral>(serde_json::json!({})).is_err());
    }

    /// La section de l'API s'écrit comme celle des droits, pour chacune des trois ; celle où
    /// l'on crée s'écrit comme elles.
    #[test]
    fn section_is_written_like_the_acl_section() {
        use mementum_acl::Section as Acl;
        for section in [Acl::Space, Acl::Shared, Acl::Private] {
            let written = serde_json::to_value(Section::from(section)).unwrap();
            assert_eq!(written, section.as_str());
        }
        for (root, section) in [
            (RootSection::Space, Acl::Space),
            (RootSection::Private, Acl::Private),
        ] {
            assert_eq!(serde_json::to_value(root).unwrap(), section.as_str());
        }
        assert_eq!(RootSection::default(), RootSection::Private);
    }

    /// La raison d'une version s'écrit comme le store la garde, pour chacune des trois.
    #[test]
    fn version_reason_is_written_like_the_store_keeps_it() {
        use mementum_store::VersionReason as Kept;
        for reason in [Kept::Periodic, Kept::Closing, Kept::BeforeRestore] {
            let written = serde_json::to_value(VersionReason::from(reason)).unwrap();
            assert_eq!(written, reason.as_str());
        }
    }

    #[test]
    fn signup_mode_has_one_spelling() {
        for mode in [SignupMode::Open, SignupMode::Invite] {
            assert_eq!(SignupMode::parse(mode.as_str()), Some(mode));
            assert_eq!(mode.as_str().parse::<SignupMode>(), Ok(mode));
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.as_str());
        }
        assert_eq!(SignupMode::default(), SignupMode::Invite);
        for bad in ["", "Open", "INVITE", "closed", " open"] {
            assert_eq!(SignupMode::parse(bad), None, "{bad:?}");
            assert!(bad.parse::<SignupMode>().is_err(), "{bad:?}");
        }
    }
}
