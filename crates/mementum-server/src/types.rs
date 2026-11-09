//! Types de l'API REST, partagés avec le client web.
//!
//! `ts-rs` les traduit en TypeScript dans `packages/protocol/src/generated/`
//! (fichiers commités). Après une modification ici :
//! `UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed`.
//!
//! Les identifiants sont des UUID écrits en texte. Les horodatages sont en
//! millisecondes Unix, des entiers sûrs en JavaScript (< 2⁵³) : ils sont
//! déclarés `number` et non `bigint`, puisque `JSON.parse` rend un nombre.

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
    /// Blocs que BlockNote refuse (structure, profondeur).
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
    /// En-tête `Origin` d'une autre page que celles de la boucle locale ou du
    /// serveur, à `/sync` (Ruling R31). Statut 403.
    BadOrigin,
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
        file::<CreatePage>(&cfg),
        file::<ErrorBody>(&cfg),
        file::<ErrorCode>(&cfg),
        file::<Info>(&cfg),
        file::<PageNode>(&cfg),
        file::<Tree>(&cfg),
        file::<UpdatePage>(&cfg),
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
