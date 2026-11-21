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

/// Les pages qui mènent à une page (`GET /api/v1/pages/{id}/backlinks`, lot 4, Task S6) :
/// seulement celles que le compte voit, de l'espace de la page, hors corbeille, dans l'ordre de
/// leurs identifiants, deux cents au plus. Rien ne dit s'il y en a d'autres, ni combien.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Backlinks {
    pub pages: Vec<Backlink>,
}

/// Une page qui mène à une autre, par un lien de page, une mention de page ou un lien du texte
/// vers son adresse ([`Backlinks`]). Son titre se lit dans l'arbre de l'espace ([`Tree`]), qui
/// montre les mêmes pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub page_id: String,
    /// Le premier de ses blocs qui y mène, dans l'ordre des identifiants de bloc (pas l'ordre du
    /// document) : un identifiant écrit par un client, une chaîne quelconque de cent octets au
    /// plus, à ne mettre dans une adresse qu'échappée. `null` : le bloc n'a pas d'identifiant qui
    /// puisse servir (absent, répété dans la page, trop long), et ne se rejoint pas.
    pub block_id: Option<String>,
    /// Le texte de ce bloc, tel que la recherche l'indexe, coupé à 240 caractères (points de code,
    /// pas unités UTF-16), sans marque de coupe : du texte, jamais du HTML. Pour un lien de page,
    /// le titre qu'il a copié ; il peut être vide.
    pub excerpt: String,
}

/// Corps de `POST /api/v1/spaces/{id}/search` (lot 4, Task S5, Ruling R917) : la recherche est
/// dans le corps, jamais dans l'adresse.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRequest {
    /// Ce que la personne a tapé, tel quel : 200 caractères et 8 mots au plus, sans aucune
    /// syntaxe (un opérateur n'est que des caractères). Le dernier mot est aussi cherché comme
    /// début de mot. Sans aucun mot, la réponse est vide.
    pub q: String,
    /// Le nombre de pages déjà reçues pour cette recherche : absent ou 0 pour la première
    /// réponse, puis 20, 40, 60, 80. À partir de 100 : `422 invalid_query`.
    #[serde(default)]
    #[ts(optional)]
    pub after: Option<u32>,
    /// Seulement les pages créées par ce compte (son identifiant) : lot 5, Ruling R1248 ; un
    /// filtre réduit l'ensemble des pages que le compte voit, jamais ne l'élargit.
    #[serde(default)]
    #[ts(optional)]
    pub created_by: Option<String>,
    /// Seulement les pages dont le contenu date d'au moins cet instant (millisecondes Unix) ; une
    /// page pas encore indexée depuis sa création n'a pas de date, et ne passe pas.
    #[serde(default)]
    #[ts(optional, type = "number")]
    pub edited_after: Option<i64>,
    /// Seulement les pages dont le contenu date d'avant cet instant (millisecondes Unix).
    #[serde(default)]
    #[ts(optional, type = "number")]
    pub edited_before: Option<i64>,
}

/// Les pages trouvées dans un espace (`POST /api/v1/spaces/{id}/search`) : seulement celles que
/// le compte voit, hors corbeille, la mieux notée d'abord, vingt au plus par réponse. Aucun total
/// (Ruling R905).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub results: Vec<SearchHit>,
    /// D'autres pages suivent : les demander avec `after` augmenté du nombre de pages reçues.
    /// Faux à la centième page, qu'il y en ait d'autres ou non.
    pub more: bool,
    /// L'index n'est pas à jour pour une page que le compte voit dans cet espace (une page vient
    /// d'être écrite, ou le serveur finit de démarrer ou de reconstruire son index) : des pages
    /// peuvent manquer, et une réponse vide n'est pas sûre. Jamais un nombre.
    pub indexing: bool,
}

/// Une page trouvée ([`SearchResults`]), par son titre ou par un de ses blocs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page_id: String,
    /// Le titre de la page à l'instant de la réponse (lu dans la base, pas dans l'index).
    pub title: String,
    /// Les pages et dossiers au-dessus d'elle **que le compte voit**, du plus haut au plus
    /// proche ; vide à la racine, ou quand il ne voit pas la page parente.
    pub path: Vec<SearchAncestor>,
    /// Le bloc trouvé : un identifiant écrit par un client, une chaîne quelconque de cent octets
    /// au plus, à ne mettre dans une adresse qu'échappée. `null` : la page est trouvée par son
    /// titre, ou le bloc n'a pas d'identifiant qui puisse servir (absent, répété, trop long).
    pub block_id: Option<String>,
    /// Un morceau du texte du bloc trouvé, 240 caractères au plus : du texte, jamais du HTML.
    /// Vide pour une page trouvée par son titre.
    pub excerpt: String,
    /// Les mots trouvés dans `excerpt` : des plages `[début, fin[` en unités UTF-16 (les places
    /// d'une chaîne JavaScript), dans l'ordre, sans chevauchement.
    pub marks: Vec<(u32, u32)>,
}

/// Une page ou un dossier au-dessus d'une page trouvée ([`SearchHit::path`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchAncestor {
    pub id: String,
    pub title: String,
}

/// Un fichier envoyé dans une page (`POST /api/v1/pages/{id}/files`, Rulings R245 à R247) : une
/// attache de la page, qui nomme un contenu gardé sous son SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    /// L'identifiant de l'attache.
    pub id: String,
    /// L'adresse du fichier, relative à l'instance : `/api/v1/pages/<page>/files/<id>`, à mettre
    /// dans la prop `url` du bloc (Ruling R248). Elle se lit avec « Peut lire » sur la page.
    pub url: String,
    /// Le nom d'affichage, nettoyé par le serveur : un libellé, jamais un chemin (255 caractères
    /// au plus, l'extension gardée, « fichier » s'il ne montre rien).
    pub name: String,
    /// Octets du contenu.
    #[ts(type = "number")]
    pub size: i64,
    /// Le type du contenu, lu par le serveur dans ses premiers octets (Ruling R246) :
    /// `image/png`, `video/mp4`… ou `application/octet-stream` pour tout ce que sa table ne
    /// connaît pas. Le type que le navigateur déclare n'est jamais lu.
    pub mime: String,
}

/// Corps de `POST /api/v1/pages/{id}/files/copies` (Ruling R249) : le fichier d'une autre page à
/// recopier dans celle-ci, sans renvoyer un octet. Les deux identifiants sont des chaînes : un
/// identifiant illisible reçoit la réponse d'une source qu'on ne voit pas (`422 invalid_source`),
/// pas celle d'un corps illisible.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopyFile {
    /// La page d'origine, qu'il faut pouvoir lire (à la corbeille aussi).
    pub page_id: String,
    /// Le fichier de la page d'origine : l'identifiant de son attache, le dernier segment de son
    /// adresse (`/api/v1/pages/<pageId>/files/<fileId>`).
    pub file_id: String,
}

/// Les bornes d'un envoi de fichier (`GET /api/v1/files/limits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileLimits {
    /// Octets au plus par fichier (`MEMENTUM_MAX_FILE_MB`, 100 Mio par défaut) : au-delà,
    /// l'envoi est refusé (`413 file_too_large`).
    #[ts(type = "number")]
    pub max_file_bytes: u64,
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
    /// Juste avant qu'un agent (un jeton d'API) remplace ce qu'une autre écriture avait changé :
    /// ce qu'il a remplacé (Ruling R1234).
    BeforeAgent,
}

impl From<mementum_store::VersionReason> for VersionReason {
    fn from(reason: mementum_store::VersionReason) -> VersionReason {
        match reason {
            mementum_store::VersionReason::Periodic => VersionReason::Periodic,
            mementum_store::VersionReason::Closing => VersionReason::Closing,
            mementum_store::VersionReason::BeforeRestore => VersionReason::BeforeRestore,
            mementum_store::VersionReason::BeforeAgent => VersionReason::BeforeAgent,
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
    /// Ceux de ces comptes qui l'ont modifiée par un jeton d'API, « par un agent » (lot 5, Ruling
    /// R1234) : leurs identifiants, dans l'ordre de `authors`, dont ils sont toujours une part ;
    /// vide pour une version sans écriture d'agent. Le jeton n'y est jamais nommé : son nom n'est
    /// montré qu'à son compte, dans son activité.
    pub agent_authors: Vec<String>,
}

/// Réponse de `POST /api/v1/auth/password` : combien de jetons d'API vivants le changement a
/// révoqués, dans sa transaction (Ruling R1221).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PasswordChanged {
    #[ts(type = "number")]
    pub tokens_revoked: u64,
}

/// Le niveau d'un jeton d'API (lot 5, Ruling R1218) : lire, ou modifier ; jamais l'accès
/// complet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TokenLevel {
    Read,
    Edit,
}

/// Ce qu'un jeton d'API ouvre (Ruling R1219) : tous les espaces de son compte, un seul, ou des
/// pages d'un seul espace avec leurs sous-pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TokenScope {
    All,
    Space,
    Pages,
}

/// Corps de `POST /api/v1/tokens` (une session seulement). `level` et `scope` sont lus comme du
/// texte, pour qu'une autre valeur reçoive `422 invalid_level` ou `invalid_scope`, et non le
/// refus d'un corps illisible.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateToken {
    /// 1 à 60 caractères, sans caractère de contrôle ; espaces de bord retirées.
    pub name: String,
    /// `read` ou `edit`.
    #[ts(as = "TokenLevel")]
    pub level: String,
    /// `all`, `space` ou `pages`.
    #[ts(as = "TokenScope")]
    pub scope: String,
    /// L'espace, pour `space` et `pages` ; absent ou `null` pour `all`.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub space_id: Option<String>,
    /// Les pages, pour `pages` seulement : 1 à 50, de l'espace nommé, chacune avec ses
    /// sous-pages.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub page_ids: Option<Vec<String>>,
    /// La corbeille permise au jeton (mettre à la corbeille et restaurer) ; absent : non.
    #[serde(default)]
    pub can_trash: bool,
    /// L'échéance, en jours depuis la création (1 à 3 650) ; absent ou `null` : aucune.
    #[serde(default)]
    #[ts(optional = nullable, type = "number | null")]
    pub expires_in_days: Option<i64>,
    /// Le mot de passe actuel du compte, redemandé (décision L2).
    pub password: String,
}

/// L'espace d'un jeton, tel que son compte le voit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenSpace {
    pub id: String,
    /// Son nom, si le compte y a encore un rôle ; sinon `null`.
    pub name: Option<String>,
}

/// Une page de la portée d'un jeton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenPage {
    pub id: String,
    /// Son titre, si le compte la voit encore (corbeille comprise) ; sinon `null`.
    pub title: Option<String>,
}

/// Un jeton d'API, tel que son compte le voit (`GET /api/v1/tokens`) : jamais son texte ni son
/// hachage, seulement ses quatre derniers caractères.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub id: String,
    pub name: String,
    /// Les quatre derniers caractères du jeton, pour le reconnaître.
    pub hint: String,
    pub level: TokenLevel,
    pub scope: TokenScope,
    /// L'espace d'une portée `space` ou `pages` ; `null` pour `all`.
    pub space: Option<TokenSpace>,
    /// Les pages d'une portée `pages`, celles qui restent (une page supprimée part de la
    /// portée) ; vide pour les autres portées.
    pub pages: Vec<TokenPage>,
    pub can_trash: bool,
    /// Millisecondes Unix, comme les dates qui suivent.
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub expires_at: Option<i64>,
    /// Le plus récent des appels écrits à son journal (un refus du débit n'y compte pas).
    #[ts(type = "number | null")]
    pub last_used_at: Option<i64>,
    #[ts(type = "number | null")]
    pub revoked_at: Option<i64>,
    /// Le jeton vaut encore à la lecture, d'après l'horloge du serveur : ni révoqué, ni échu.
    pub live: bool,
    /// Ses appels refusés par le débit (`429 token_busy`), comptés sans ligne au journal.
    #[ts(type = "number")]
    pub busy_count: i64,
    /// Les lignes de son journal que la purge a retirées (les mille derniers appels réussis et
    /// les deux cents derniers autres restent).
    #[ts(type = "number")]
    pub calls_purged: i64,
}

/// Réponse de `POST /api/v1/tokens` : le jeton, **montré cette fois seulement** (`mmt_` et 43
/// caractères), et ce que la liste en montrera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenCreated {
    pub token: String,
    pub info: TokenInfo,
}

/// Par où un appel par jeton est venu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TokenCallSurface {
    Mcp,
    Rest,
}

/// L'issue d'un appel par jeton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TokenCallOutcome {
    Ok,
    Refused,
    Error,
}

/// Une ligne du journal d'activité d'un jeton (Ruling R1245) : ni texte cherché, ni MMD, ni
/// texte remplacé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenCall {
    /// Son rang dans le journal : `before` de la page suivante.
    #[ts(type = "number")]
    pub id: i64,
    /// Millisecondes Unix.
    #[ts(type = "number")]
    pub at: i64,
    pub surface: TokenCallSurface,
    /// L'outil MCP, ou la route (« MÉTHODE chemin déclaré »).
    pub tool: String,
    pub space_id: Option<String>,
    pub page_id: Option<String>,
    /// Le titre de la page, si le compte la voit encore (corbeille comprise) ; sinon `null`.
    pub page_title: Option<String>,
    pub outcome: TokenCallOutcome,
    /// Le code du refus ou de l'erreur.
    pub code: Option<String>,
    pub revision_before: Option<String>,
    pub revision_after: Option<String>,
    #[ts(type = "number | null")]
    pub blocks_touched: Option<i64>,
}

/// Réponse de `GET /api/v1/tokens/{id}/calls` : cinquante lignes au plus, de la plus récente à la
/// plus ancienne.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenCalls {
    pub calls: Vec<TokenCall>,
    /// Des lignes plus anciennes restent : les demander avec `before`, l'`id` de la dernière.
    pub more: bool,
    /// Ses appels refusés par le débit, comptés sans ligne.
    #[ts(type = "number")]
    pub busy_count: i64,
    /// Les lignes que la purge a retirées.
    #[ts(type = "number")]
    pub purged: i64,
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
    /// La restauration d'une version est refusée, rien n'est changé : l'état de la version ne se
    /// relit pas, ou celui de la page, qu'aucune version ne garderait (Rulings R816, R832). Le
    /// message dit lequel. Statut 409.
    RestoreRefused,
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
    /// La requête nomme un compte (en-tête `X-Mementum-Account`, ou paramètre `account` d'une
    /// WebSocket) qui n'est pas celui de sa session : un autre compte s'est connecté dans ce
    /// navigateur depuis. Rien n'est lu ni écrit (Ruling R540). Statut 409.
    AccountChanged,
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
    /// `PUT /mmd` sur une page dont la file des écritures est pleine (Ruling R435) : réessayer
    /// après le délai de l'en-tête `Retry-After` ; rien n'est écrit. Statut 429.
    PageBusy,
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
    /// Fichier plus gros que la borne de l'instance (`MEMENTUM_MAX_FILE_MB`, 100 Mio par défaut),
    /// refusé sur `Content-Length` avant d'être lu, ou en cours de lecture : rien n'est gardé.
    /// Statut 413.
    FileTooLarge,
    /// Plage (`Range`) qui commence au-delà de la fin du fichier ; l'en-tête `Content-Range`
    /// donne sa taille (`bytes */<taille>`). Statut 416.
    RangeNotSatisfiable,
    /// La source d'une recopie de fichier ne désigne pas un fichier d'une page que le compte peut
    /// lire : la même réponse pour une page qu'il ne voit pas, inconnue ou mal nommée, et pour un
    /// fichier qui n'est pas de cette page, inconnu ou mal nommé (Ruling R558). Statut 422.
    InvalidSource,
    /// La page à dupliquer et ses sous-pages font plus de cent pages, ou leurs documents pèsent
    /// trop pour une seule écriture (Ruling R252) ; le message dit lequel : rien n'est copié, et
    /// l'on duplique par morceaux. Statut 422.
    SubtreeTooLarge,
    /// Le compte a déjà une duplication en cours, ou a dupliqué trop de contenu dans la dernière
    /// minute (Rulings R605, R606) ; le message dit lequel, et l'en-tête `Retry-After` quand
    /// revenir : rien n'est copié, et la demande n'attend pas. Statut 429.
    DuplicationBusy,
    /// La recherche fait plus de 200 caractères ou de 8 mots, ou demande au-delà de ses cent
    /// premières pages (Ruling R911) ; le message dit lequel. Statut 422.
    InvalidQuery,
    /// Une recherche plus récente du même compte a pris la place de celle-ci, qui attendait
    /// (Ruling R910) : rien n'a été cherché pour elle, et sa réponse ne sert plus. L'en-tête
    /// `Retry-After` dit quand réessayer. Statut 429.
    SearchBusy,
    /// `Authorization: Bearer mmt_…` qui ne porte pas un jeton d'API vivant : mal formé, inconnu,
    /// révoqué, échu, ou d'un compte qui n'existe plus ; la même réponse dans tous les cas, avec
    /// `WWW-Authenticate: Bearer` (lot 5, Ruling R1215). Statut 401.
    InvalidToken,
    /// Un jeton d'API sur une route qui ne s'ouvre qu'à une session (lot 5, Ruling R1222) : rien
    /// n'est lu ni écrit. Statut 403.
    TokenNotAllowed,
    /// Le jeton, ou l'ensemble des jetons de son compte, a fait trop d'appels, ou envoyé trop
    /// d'octets, dans la dernière minute (lot 5, Ruling R1241) : l'en-tête `Retry-After` dit quand
    /// réessayer ; rien n'est fait. Statut 429.
    TokenBusy,
    /// Le compte a déjà vingt jetons d'API vivants (lot 5, Ruling R1220) : en révoquer un avant
    /// d'en créer un autre. Rien n'est créé. Statut 409.
    TokenLimit,
    /// La portée d'un jeton à créer ne se lit pas, ou nomme un espace où l'on n'a aucun rôle, ou
    /// une page qu'on ne voit pas (ou à la corbeille) : la même réponse qu'un espace ou une page
    /// inconnus. Statut 422.
    InvalidScope,
    /// Le niveau d'un jeton à créer n'est ni `read` ni `edit` (Ruling R1218). Statut 422.
    InvalidLevel,
    /// L'échéance d'un jeton à créer n'est pas un nombre de jours de 1 à 3 650. Statut 422.
    InvalidExpiry,
    /// Le mode d'écriture demandé (`suggest`, `branch`) n'est pas encore servi : seul `direct`
    /// l'est (lot 5, Ruling R1211). Rien n'est écrit. Statut 422.
    ModeUnavailable,
    /// L'opération remplace ou retire des blocs (`replace_page`, `replace_block`,
    /// `delete_block`) : elle demande la révision de la dernière lecture (Ruling R1231). Rien
    /// n'est écrit. Statut 422.
    RevisionRequired,
    /// Plus de cent opérations, ou aucune, dans un appel. Statut 422.
    TooManyOperations,
    /// Une opération ne s'applique pas (`index` : son rang ; `reason` : `no_match`,
    /// `stale_anchor`, `invalid_markdown`…) ; rien n'est écrit (Ruling R1229). Statut 422.
    OperationFailed,
    /// Une écriture par jeton d'API poserait une image, une vidéo ou un son à adresse web
    /// (Ruling R1235). Rien n'est écrit. Statut 422.
    ExternalMedia,
    /// La page n'est plus à la révision nommée, et les blocs en cause ont changé (Ruling R1231) :
    /// `revision` porte la révision courante. Rien n'est écrit. Statut 409.
    RevisionConflict,
    /// Quelqu'un écrit dans la page en ce moment : `replace_page` par un jeton est refusé
    /// (Ruling R1233). Rien n'est écrit. Statut 409.
    PageInUse,
    /// La version qui garde ce qu'une opération d'agent remplace n'a pas pu être écrite (Ruling
    /// R1234). Rien n'est écrit. Statut 409.
    VersionRefused,
    /// Un lot de pages à créer en porte plus de vingt, ou aucune. Statut 422.
    TooManyPages,
    /// Un filtre de recherche ne se lit pas (lot 5, Ruling R1248). Statut 422.
    InvalidFilter,
    /// Une ancre (un alias de bloc, ou un identifiant) ne désigne aucun bloc de la page, ou en
    /// désigne plusieurs : relire la page avec ses ancres. Statut 422.
    StaleAnchor,
}

// ================================================================== agents (lot 5, Task S7)
//
// Ce que lisent et envoient les agents, par jeton d'API ou par session : aucun écran ne s'en
// sert, si bien que ces types n'ont pas de binding TypeScript.

/// Une page lue par un agent (`GET /api/v1/pages/{id}/document`, Rulings R1223, R1225, R1226) :
/// ses métadonnées, sa révision, et une tranche de son MMD, 48 Kio au plus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDocument {
    pub id: String,
    pub space_id: String,
    pub title: String,
    /// La page parente, si le compte (restreint au jeton) la voit.
    pub parent_id: Option<String>,
    /// Les pages au-dessus, que le compte voit, du plus haut au plus proche (le fil d'Ariane de
    /// la recherche).
    pub path: Vec<SearchAncestor>,
    /// Le niveau de l'appel sur la page (celui du compte, ramené sous celui du jeton).
    pub level: Level,
    /// Millisecondes Unix : la date du contenu (`page_edits`, Ruling R1246) ; `null` tant que
    /// l'indexeur n'a pas lu la page depuis sa création.
    pub edited_at: Option<i64>,
    /// La révision des blocs rendus, sortie de la même commande que leur lecture (Ruling R1226) :
    /// celle à donner à une écriture, pour la **dernière** tranche lue.
    pub revision: String,
    /// Le MMD de la tranche, `limit` octets au plus.
    pub mmd: String,
    /// D'autres blocs suivent : les lire par `from=<next>&since=<révision de la première
    /// tranche>`.
    pub truncated: bool,
    /// L'alias du premier bloc non rendu.
    pub next: Option<String>,
    /// Les sous-pages, que le compte voit, hors corbeille, dans l'ordre : deux cents au plus.
    pub children: Vec<SearchAncestor>,
    /// Avec `since` : quelque chose a changé, depuis cette révision, dans les blocs déjà servis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_since: Option<bool>,
    /// Avec `since` : les alias des blocs déjà servis qui ont changé, sont partis, ou après
    /// lesquels un bloc est arrivé ; `null` quand le serveur n'a plus les empreintes de `since`
    /// (relire depuis le début).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<Option<Vec<String>>>,
}

/// Corps de `POST /api/v1/pages/{id}/operations` (Rulings R1211, R1228 à R1235) : les opérations
/// sont celles de l'outil `update_page`, sous la même forme (`mementum_mcp::tools::Operation`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageOperations {
    /// La révision de la dernière lecture ; exigée par `replace_page`, `replace_block` et
    /// `delete_block`.
    #[serde(default)]
    pub base_revision: Option<String>,
    /// `rebase` (par défaut) ou `fail`.
    #[serde(default)]
    pub on_conflict: Option<mementum_mcp::tools::OnConflict>,
    /// `direct` (par défaut, et le seul servi).
    #[serde(default)]
    pub mode: Option<mementum_mcp::tools::Mode>,
    pub operations: Vec<mementum_mcp::tools::Operation>,
}

/// Réponse de `POST /api/v1/pages/{id}/operations`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    /// La révision de l'état écrit, sortie de la commande qui écrit (RESULTS, leçon 8).
    pub revision: String,
    /// Le MMD des blocs touchés, avec leurs ancres, 48 Kio au plus.
    pub mmd: String,
    pub truncated: bool,
    /// Les blocs dont un élément a changé.
    pub blocks_touched: usize,
    /// Toujours vide au lot 5 : un appel est atomique, une opération qui ne passe pas est un
    /// refus (Ruling R1229).
    pub warnings: Vec<String>,
}

/// Corps de `POST /api/v1/spaces/{id}/pages/batch` (Ruling R1243) : des pages créées ensemble,
/// avec leur contenu, toutes ou aucune.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePages {
    /// La page sous laquelle les créer ; absent : à la racine de l'espace.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// À la racine seulement : `private` (par défaut) ou `space`.
    #[serde(default)]
    pub section: Option<RootSection>,
    /// Vingt au plus.
    pub pages: Vec<NewPageBody>,
}

/// Une page d'un lot ([`CreatePages`]).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewPageBody {
    /// Absent ou blanc : « Sans titre » ; 500 caractères au plus.
    #[serde(default)]
    pub title: Option<String>,
    /// Le contenu, en MMD canonique ; absent : une page vide.
    #[serde(default)]
    pub markdown: Option<String>,
}

/// `GET /api/v1/me` par un jeton d'API (Ruling R1247) : le compte **sans son adresse**, les
/// espaces de la portée du jeton, et le jeton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMe {
    pub user: AgentUser,
    pub spaces: Vec<SpaceInfo>,
    pub token: TokenInfo,
}

/// Le compte d'un jeton, tel qu'un agent le lit : jamais son adresse e-mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUser {
    pub id: String,
    pub name: String,
}

/// Un membre d'un espace, tel qu'un agent le lit (`GET …/members` par un jeton, Ruling R1247) :
/// son nom et son rôle, jamais son adresse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberName {
    pub user_id: String,
    pub name: String,
    pub role: Role,
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
        file::<Backlink>(&cfg),
        file::<Backlinks>(&cfg),
        file::<ChangePassword>(&cfg),
        file::<Comment>(&cfg),
        file::<CopyFile>(&cfg),
        file::<CreatePage>(&cfg),
        file::<CreateSpace>(&cfg),
        file::<CreateToken>(&cfg),
        file::<ErrorBody>(&cfg),
        file::<ErrorCode>(&cfg),
        file::<Favorites>(&cfg),
        file::<FileInfo>(&cfg),
        file::<FileLimits>(&cfg),
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
        file::<PasswordChanged>(&cfg),
        file::<PendingInvitation>(&cfg),
        file::<Reaction>(&cfg),
        file::<ResolveThread>(&cfg),
        file::<Role>(&cfg),
        file::<RootSection>(&cfg),
        file::<SearchAncestor>(&cfg),
        file::<SearchHit>(&cfg),
        file::<SearchRequest>(&cfg),
        file::<SearchResults>(&cfg),
        file::<Section>(&cfg),
        file::<SetGeneral>(&cfg),
        file::<SetLevel>(&cfg),
        file::<SignUp>(&cfg),
        file::<SignupMode>(&cfg),
        file::<SpaceInfo>(&cfg),
        file::<Thread>(&cfg),
        file::<TokenCall>(&cfg),
        file::<TokenCallOutcome>(&cfg),
        file::<TokenCallSurface>(&cfg),
        file::<TokenCalls>(&cfg),
        file::<TokenCreated>(&cfg),
        file::<TokenInfo>(&cfg),
        file::<TokenLevel>(&cfg),
        file::<TokenPage>(&cfg),
        file::<TokenScope>(&cfg),
        file::<TokenSpace>(&cfg),
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

    /// La raison d'une version s'écrit comme le store la garde, pour chacune des quatre.
    #[test]
    fn version_reason_is_written_like_the_store_keeps_it() {
        use mementum_store::VersionReason as Kept;
        for reason in [
            Kept::Periodic,
            Kept::Closing,
            Kept::BeforeRestore,
            Kept::BeforeAgent,
        ] {
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
