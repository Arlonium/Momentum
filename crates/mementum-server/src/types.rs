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

/// Une page de l'arbre (`GET /api/v1/tree`, réponses de `POST` et `PATCH`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PageNode {
    pub id: String,
    /// `null` : la page est à la racine du coffre.
    pub parent_id: Option<String>,
    /// `"page"` (le lot 1 ne crée que des pages).
    pub kind: String,
    pub title: String,
    /// Index fractionnaire : l'ordre des frères est l'ordre de ces chaînes,
    /// comparées unité par unité (ASCII).
    pub position: String,
    /// Millisecondes Unix.
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// Les pages du coffre (`GET /api/v1/tree`), triées par `(parentId,
/// position)`, la racine d'abord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub vault_id: String,
    pub nodes: Vec<PageNode>,
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

/// Corps de `POST /api/v1/pages`. Tous les champs sont facultatifs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePage {
    /// Absent ou `null` : à la racine.
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

/// Code d'une erreur de l'API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Page (ou route) introuvable.
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
    /// `afterId` ne désigne pas un frère à l'endroit visé.
    InvalidAfter,
    /// `parentId` ne désigne pas une page du coffre.
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
        file::<ChangePassword>(&cfg),
        file::<CreatePage>(&cfg),
        file::<ErrorBody>(&cfg),
        file::<ErrorCode>(&cfg),
        file::<Info>(&cfg),
        file::<LogIn>(&cfg),
        file::<Me>(&cfg),
        file::<PageNode>(&cfg),
        file::<Role>(&cfg),
        file::<SignUp>(&cfg),
        file::<SignupMode>(&cfg),
        file::<SpaceInfo>(&cfg),
        file::<Tree>(&cfg),
        file::<UpdatePage>(&cfg),
        file::<UserInfo>(&cfg),
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
