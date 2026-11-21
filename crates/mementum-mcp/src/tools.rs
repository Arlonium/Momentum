//! Les six outils : leurs types d'entrée (dont les schémas sont dérivés, Ruling R1238), leurs
//! plafonds, leurs descripteurs dans un ordre fixe, et la lecture des arguments d'un appel.
//!
//! Les noms des outils et de leurs champs sont ceux de la conception (§6.3, §6.5), en anglais ;
//! les descriptions sont en français (Ruling R1240). Aucune ne porte la spécification du MMD :
//! elle est dans la ressource `mementum://spec/mmd` (§6.2).

use std::sync::OnceLock;

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::protocol::Endpoint;
use crate::text::{self, ToolOutput};

/// Caractères d'une requête de recherche, au plus (`MAX_QUERY_CHARS` de `mementum-search`).
pub const MAX_QUERY_CHARS: usize = 200;
/// Caractères d'un nom d'espace ou d'un nom de personne cherché, au plus.
pub const MAX_NAME_CHARS: usize = 200;
/// Caractères d'un titre de page, au plus (`MAX_TITLE_CHARS` de `routes/pages.rs`).
pub const MAX_TITLE_CHARS: usize = 500;
/// Caractères d'une désignation de page (identifiant ou adresse), au plus.
pub const MAX_PAGE_REF_CHARS: usize = 2_048;
/// Caractères d'une ancre de bloc (un alias, ou l'identifiant entier d'un bloc), au plus.
pub const MAX_ANCHOR_CHARS: usize = 120;
/// Caractères d'une révision, d'une date ou d'un curseur, au plus.
pub const MAX_TOKEN_CHARS: usize = 120;
/// Caractères d'un `old` de `str_replace` (Ruling R1228).
pub const MAX_OLD_CHARS: usize = 2_000;
/// Pages d'un `create_pages`, au plus.
pub const MAX_PAGES: usize = 20;
/// Caractères d'un MMD donné à un outil, au plus : un champ `markdown`, et tous ceux d'un
/// `create_pages` ensemble. Sous la borne d'un corps ([`crate::protocol::MAX_BODY_BYTES`], 2 Mio),
/// l'enveloppe JSON-RPC et les autres champs comptés (Task M2) : un MMD plus long ne tiendrait
/// pas dans le corps.
pub const MAX_MARKDOWN_CHARS: usize = 2_000_000;
/// Opérations d'un `update_page`, au plus (§6.3).
pub const MAX_OPERATIONS: usize = 100;
/// Octets de MMD d'une lecture : par défaut, et au plus (Ruling R1225).
pub const DEFAULT_LIMIT: u32 = 24 * 1024;
pub const MAX_LIMIT: u32 = 48 * 1024;

/// `search` : chercher des pages par leur texte.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    /// Les mots cherchés : 200 caractères au plus, 8 mots au plus (une lettre seule avant le
    /// dernier mot et un mot répété ne comptent pas) ; au-delà, la recherche est refusée.
    #[schemars(length(min = 1, max = MAX_QUERY_CHARS))]
    pub query: String,
    /// L'espace où chercher, par son nom ou son identifiant. Facultatif si le jeton n'ouvre qu'un
    /// espace ; exigé sinon (`workspace_info` les nomme).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_NAME_CHARS))]
    pub space: Option<String>,
    /// Seulement les pages créées par cette personne (son nom, tel que `workspace_info` le
    /// donne, ou l'identifiant de son compte).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_NAME_CHARS))]
    pub created_by: Option<String>,
    /// Seulement les pages dont le contenu a changé depuis cette date (`AAAA-MM-JJ`, ou date et
    /// heure RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_TOKEN_CHARS))]
    pub edited_after: Option<String>,
    /// Seulement les pages dont le contenu n'a pas changé depuis cette date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_TOKEN_CHARS))]
    pub edited_before: Option<String>,
    /// La suite d'une recherche : le nombre que donne la ligne « Suite » d'une réponse précédente
    /// (`after=N`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_TOKEN_CHARS))]
    pub after: Option<String>,
}

/// `fetch` : lire une page, ou le plan d'un espace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchInput {
    /// Ce qu'on lit : l'identifiant ou l'adresse d'une page (son `#b=<bloc>` fait commencer la
    /// lecture à ce bloc), ou le nom ou l'identifiant d'un espace (son plan).
    #[schemars(length(min = 1, max = MAX_PAGE_REF_CHARS))]
    pub id: String,
    /// Écrire l'alias de chaque bloc en ancre (`^bk7qa`), pour viser un bloc dans `update_page`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchors: Option<bool>,
    /// Reprendre une lecture coupée : l'alias `next` de la tranche précédente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
    pub from: Option<String>,
    /// Avec `from` : la révision de la première tranche, pour savoir ce qui a changé depuis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_TOKEN_CHARS))]
    pub since: Option<String>,
    /// Octets de MMD rendus au plus : 24 576 par défaut, de 1 024 à 49 152 (moins de 1 024 vaut
    /// 1 024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = MAX_LIMIT))]
    pub limit: Option<u32>,
}

/// La section d'une page créée à la racine d'un espace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// « Privé » : la page n'est vue que par toi.
    Private,
    /// « Espace » : la page est vue par les membres de l'espace.
    Space,
}

/// Une page à créer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewPage {
    /// Son titre : 500 caractères au plus (vide : « Sans titre »).
    #[schemars(length(max = MAX_TITLE_CHARS))]
    pub title: String,
    /// Son contenu, en MMD (voir la ressource `mementum://spec/mmd`) ; 2 000 000 caractères au
    /// plus, pour toutes les pages de l'appel ensemble.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = MAX_MARKDOWN_CHARS))]
    pub markdown: Option<String>,
}

/// `create_pages` : créer des pages, sous une page ou à la racine d'un espace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePagesInput {
    /// L'espace, par son nom ou son identifiant, pour créer à sa racine. Facultatif si le jeton
    /// n'ouvre qu'un espace, ou si `parent` est donné.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_NAME_CHARS))]
    pub space: Option<String>,
    /// La page sous laquelle créer, par son identifiant ou son adresse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_PAGE_REF_CHARS))]
    pub parent: Option<String>,
    /// À la racine : `private` (par défaut) ou `space`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<Section>,
    /// Les pages, 20 au plus, créées toutes ou aucune.
    #[schemars(length(min = 1, max = MAX_PAGES))]
    pub pages: Vec<NewPage>,
}

/// Que faire quand la page a changé depuis `base_revision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    /// Écrire si les blocs touchés n'ont pas changé (par défaut).
    Rebase,
    /// Refuser dès que la page a changé.
    Fail,
}

/// Le mode d'écriture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Écrire tout de suite (le seul mode servi aujourd'hui, et le défaut).
    Direct,
    /// Proposer sans écrire : pas encore servi.
    Suggest,
    /// Écrire dans une branche : pas encore servi.
    Branch,
}

/// Une place dans la page, au début ou à la fin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    Start,
    End,
}

/// Les propriétés d'une page qu'on peut changer : le titre seul aujourd'hui.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Properties {
    /// Le nouveau titre : 500 caractères au plus.
    #[schemars(length(max = MAX_TITLE_CHARS))]
    pub title: String,
}

/// Une opération d'`update_page` (§6.3). Les ancres sont les alias rendus par `fetch` avec
/// `anchors` (`bk7qa`), ou l'identifiant entier d'un bloc.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Remplacer un texte par un autre, dans le texte brut d'un bloc (jamais à cheval sur deux
    /// blocs ; ce qu'est ce texte brut est dit à la section « Ce que `str_replace` cherche » de la
    /// ressource `mementum://spec/mmd`). `old` doit se trouver une seule fois dans la page, ou dans
    /// le bloc `anchor`, sauf `replace_all`. `new` est du texte brut, qui prend la mise en forme
    /// du premier caractère remplacé.
    StrReplace {
        #[schemars(length(min = 1, max = MAX_OLD_CHARS))]
        old: String,
        new: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replace_all: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        anchor: Option<String>,
    },
    /// Insérer des blocs (MMD) après ou avant un bloc, ou au début ou à la fin de la page : un
    /// seul de `after`, `before`, `position`.
    Insert {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        after: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Position>,
        #[schemars(length(max = MAX_MARKDOWN_CHARS))]
        markdown: String,
    },
    /// Remplacer un bloc et ses sous-blocs par des blocs (MMD). Demande `base_revision`.
    ReplaceBlock {
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        anchor: String,
        #[schemars(length(max = MAX_MARKDOWN_CHARS))]
        markdown: String,
        /// Permettre de retirer un lien de sous-page, une base, un bloc synchronisé ou un bloc
        /// inconnu.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_deleting_content: Option<bool>,
    },
    /// Retirer un bloc et ses sous-blocs. Demande `base_revision`.
    DeleteBlock {
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        anchor: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_deleting_content: Option<bool>,
    },
    /// Déplacer un bloc parmi les blocs de la page : un seul de `after`, `before`, `position`.
    MoveBlock {
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        anchor: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        after: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(length(min = 1, max = MAX_ANCHOR_CHARS))]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<Position>,
    },
    /// Récrire toute la page (MMD) : seulement pour une page neuve ou petite. Demande
    /// `base_revision` ; refusé (`page_in_use`) tant qu'une connexion qui peut écrire est ouverte
    /// sur la page, celle d'un onglet où une personne l'a ouverte par exemple.
    ReplacePage {
        #[schemars(length(max = MAX_MARKDOWN_CHARS))]
        markdown: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_deleting_content: Option<bool>,
    },
    /// Changer les propriétés de la page : le titre seul aujourd'hui.
    SetProperties { properties: Properties },
}

/// `update_page` : changer le contenu d'une page, par une liste d'opérations atomique.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePageInput {
    /// La page, par son identifiant ou son adresse.
    #[schemars(length(min = 1, max = MAX_PAGE_REF_CHARS))]
    pub page: String,
    /// La révision de la dernière lecture (`fetch`). Exigée par `replace_page`, `replace_block`
    /// et `delete_block`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_TOKEN_CHARS))]
    pub base_revision: Option<String>,
    /// `rebase` (par défaut) ou `fail`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_conflict: Option<OnConflict>,
    /// `direct` (par défaut, le seul servi aujourd'hui).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// Les opérations, 100 au plus : toutes s'appliquent, ou aucune.
    #[schemars(length(min = 1, max = MAX_OPERATIONS))]
    pub operations: Vec<Operation>,
}

/// Le geste de `manage_pages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageAction {
    /// Changer de place parmi les pages sœurs (jamais sous une autre page).
    Move,
    /// Changer le titre (`title`).
    Rename,
    /// Dupliquer la page et ses sous-pages.
    Duplicate,
    /// Mettre à la corbeille (si le jeton a l'option corbeille).
    Trash,
    /// Sortir de la corbeille (si le jeton a l'option corbeille).
    Restore,
}

/// `manage_pages` : déplacer, renommer, dupliquer, mettre à la corbeille, restaurer une page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagePagesInput {
    pub action: PageAction,
    /// La page, par son identifiant ou son adresse.
    #[schemars(length(min = 1, max = MAX_PAGE_REF_CHARS))]
    pub page: String,
    /// Pour `move` : la page sœur après laquelle la placer ; sans `after`, elle passe en premier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_PAGE_REF_CHARS))]
    pub after: Option<String>,
    /// Pour `rename` : le nouveau titre, 500 caractères au plus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = MAX_TITLE_CHARS))]
    pub title: Option<String>,
}

/// `workspace_info` : qui je suis, ce que mon jeton ouvre.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInfoInput {
    /// Un espace, par son nom ou son identifiant : ses membres et ses pages de premier niveau.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1, max = MAX_NAME_CHARS))]
    pub space: Option<String>,
}

/// Un appel d'outil dont les arguments sont lus et bornés.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolCall {
    Search(SearchInput),
    Fetch(FetchInput),
    CreatePages(CreatePagesInput),
    UpdatePage(UpdatePageInput),
    ManagePages(ManagePagesInput),
    WorkspaceInfo(WorkspaceInfoInput),
}

/// Le descripteur d'un outil.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Lit sans rien changer : servi aussi par `/mcp/readonly`.
    pub read_only: bool,
    /// Peut retirer ou remplacer ce qui existe.
    pub destructive: bool,
    /// Son schéma d'entrée, dérivé de son type.
    pub input_schema: Value,
    /// Des arguments valables, montrés quand un appel est refusé.
    pub example: Value,
}

impl Tool {
    /// Le descripteur tel que `tools/list` le rend.
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.input_schema,
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
                "idempotentHint": self.read_only,
                "openWorldHint": false,
            },
        })
    }
}

/// Le schéma d'entrée d'un type : JSON Schema 2020-12 (le défaut de MCP), sous-schémas recopiés
/// en place (ni `$ref` ni `$defs`, que des clients lisent mal), sans `$schema` ni `title`.
fn schema<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::draft2020_12()
        .with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        })
        .into_generator();
    let mut schema = generator.into_root_schema_for::<T>().to_value();
    if let Value::Object(map) = &mut schema {
        map.remove("title");
        map.remove("$schema");
    }
    unwrap_descriptions(&mut schema);
    schema
}

/// Une description vient d'un commentaire de documentation coupé à cent colonnes : ses fins de
/// ligne deviennent des espaces.
fn unwrap_descriptions(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                match value {
                    Value::String(text) if key == "description" => {
                        *text = text.replace('\n', " ");
                    }
                    _ => unwrap_descriptions(value),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(unwrap_descriptions),
        _ => {}
    }
}

/// Ce qui décrit un outil, hors de son schéma.
struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
    schema: fn() -> Value,
    example: &'static str,
}

/// Les six outils, dans l'ordre de la conception (§6.5), qui est l'ordre de `tools/list`.
const SPECS: [Spec; 6] = [
    Spec {
        name: "search",
        title: "Chercher des pages",
        description: "Cherche des pages par leur texte, dans un espace que ton jeton ouvre. Rend, \
                      pour chaque page trouvée, son titre, son fil d'Ariane, un extrait et l'alias \
                      du bloc trouvé (lis la page ensuite par fetch). 20 pages par réponse ; \
                      la ligne « Suite » donne le curseur de la suite (`after`). Filtres : \
                      created_by, edited_after, edited_before.",
        read_only: true,
        destructive: false,
        schema: schema::<SearchInput>,
        example: r#"{"query":"compte rendu de lancement","space":"Pro"}"#,
    },
    Spec {
        name: "fetch",
        title: "Lire une page",
        description: "Lit une page en MMD, avec sa révision, ou le plan d'un espace. Le format est \
                      décrit par la ressource mementum://spec/mmd. Une page longue est rendue par \
                      tranches : reprends avec `from` (l'alias `next`) et `since` (la révision de \
                      la première tranche) ; une tranche dit alors quels blocs déjà lus ont changé \
                      depuis (relis-les avant de les modifier). Donne la révision de la dernière \
                      tranche à update_page. `anchors: true` écrit l'alias de chaque bloc \
                      (`^bk7qa`), pour viser un bloc. Les tranches mises bout à bout ne sont pas \
                      le MMD de la page (sous-blocs remontés, marques <unloaded>) : pour changer \
                      une longue page, vise ses blocs (str_replace, insert…), jamais \
                      replace_page. `id` prend aussi l'adresse d'une page, ou le nom d'un \
                      espace.",
        read_only: true,
        destructive: false,
        schema: schema::<FetchInput>,
        example: r#"{"id":"0193a1b2-0000-7000-8000-000000000003","anchors":true}"#,
    },
    Spec {
        name: "create_pages",
        title: "Créer des pages",
        description: "Crée des pages (20 au plus), leur contenu en MMD, sous une page ou à la \
                      racine d'un espace (section `private` par défaut, ou `space` ; `section` ne \
                      se donne pas avec `parent`). Toutes sont créées, ou aucune. Les images, \
                      vidéos et sons du web ne sont pas admis. Refus : un titre de plus de 500 \
                      caractères (title_too_long), plus de 16 Mio de documents écrits \
                      (subtree_too_large : découpe en plusieurs appels), la page parente déplacée \
                      pendant l'appel (page_moved : relis-la, puis recommence). Si \
                      la réponse ne t'arrive pas, cherche les pages (search) avant de réessayer : \
                      elles existent peut-être déjà.",
        read_only: false,
        destructive: false,
        schema: schema::<CreatePagesInput>,
        example: "{\"space\":\"Pro\",\"pages\":[{\"title\":\"Compte rendu du 20 novembre\",\
                  \"markdown\":\"# Décisions\\n\\n- Sortir la v1 avant décembre.\\n\"}]}",
    },
    Spec {
        name: "update_page",
        title: "Modifier une page",
        description: "Change le contenu d'une page par une liste d'opérations (100 au plus), \
                      appliquées toutes ou aucune : str_replace (remplacer un texte, l'opération \
                      à préférer), insert, replace_block, delete_block, move_block, replace_page \
                      (page neuve ou petite seulement) et set_properties (le titre). Donne \
                      `base_revision`, la révision de ta dernière lecture : si les blocs que tu \
                      touches ont changé depuis, l'appel est refusé (revision_conflict), avec les \
                      blocs à jour. Le seul mode servi aujourd'hui est `direct` : ce que tu écris \
                      est écrit tout de suite. Avant qu'un replace_page, un replace_block ou un \
                      delete_block remplace ce que quelqu'un d'autre a écrit depuis la dernière \
                      version, l'historique de la page en coupe une qui le garde ; le texte qu'un \
                      str_replace remplace n'est gardé par aucune version. Le défaut de `mode` \
                      deviendra `suggest` quand les \
                      suggestions seront servies : envoie `mode: \"direct\"` si tu tiens à écrire \
                      tout de suite.",
        read_only: false,
        destructive: true,
        schema: schema::<UpdatePageInput>,
        example: r#"{"page":"0193a1b2-0000-7000-8000-000000000003","base_revision":"r_0123456789abcdef","operations":[{"op":"str_replace","old":"avant décembre","new":"avant le 1er décembre"}]}"#,
    },
    Spec {
        name: "manage_pages",
        title: "Ranger une page",
        description: "Déplace une page parmi ses pages sœurs (jamais sous une autre page), la \
                      renomme, la duplique avec ses sous-pages, la met à la corbeille ou l'en \
                      sort (l'un et l'autre si ton jeton a l'option corbeille). Une page par \
                      appel.",
        read_only: false,
        destructive: true,
        schema: schema::<ManagePagesInput>,
        example: r#"{"action":"rename","page":"0193a1b2-0000-7000-8000-000000000003","title":"Compte rendu (relu)"}"#,
    },
    Spec {
        name: "workspace_info",
        title: "Qui suis-je",
        description: "Dit qui tu es, ce que ton jeton permet (niveau, portée, échéance) et les \
                      espaces qu'il ouvre ; avec `space`, les membres de cet espace (nom et rôle) \
                      et ses pages de premier niveau. Commence par lui pour trouver le nom ou \
                      l'identifiant d'un espace.",
        read_only: true,
        destructive: false,
        schema: schema::<WorkspaceInfoInput>,
        example: r#"{"space":"Pro"}"#,
    },
];

/// Les descripteurs, bâtis une fois (les schémas se dérivent à la première demande).
fn all() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        SPECS
            .iter()
            .map(|spec| Tool {
                name: spec.name,
                title: spec.title,
                description: spec.description,
                read_only: spec.read_only,
                destructive: spec.destructive,
                input_schema: (spec.schema)(),
                example: serde_json::from_str(spec.example).expect("exemple en JSON"),
            })
            .collect()
    })
}

/// Les outils que sert l'adresse, dans un ordre fixe.
pub fn list(endpoint: Endpoint) -> Vec<Tool> {
    all()
        .iter()
        .filter(|tool| endpoint == Endpoint::Full || tool.read_only)
        .cloned()
        .collect()
}

/// L'outil de ce nom, s'il est servi par l'adresse.
pub fn find(name: &str, endpoint: Endpoint) -> Option<Tool> {
    list(endpoint).into_iter().find(|tool| tool.name == name)
}

/// Le contenu de `spec/mcp/tools.json` : les descripteurs de `/mcp`, et ceux que sert
/// `/mcp/readonly`.
pub fn catalog() -> Value {
    let tools: Vec<Value> = list(Endpoint::Full).iter().map(Tool::to_json).collect();
    let readonly: Vec<&str> = list(Endpoint::ReadOnly).iter().map(|t| t.name).collect();
    json!({
        "description": "Les outils du serveur MCP de Mementum, tels que tools/list les rend sur \
                        /mcp ; /mcp/readonly ne sert que ceux de « readonly ». Généré par \
                        mementum_mcp::tools::catalog : ne pas modifier à la main ; \
                        UPDATE_TOOLS=1 cargo test -p mementum-mcp --test protocol \
                        tool_schemas_are_the_committed_ones le récrit.",
        "readonly": readonly,
        "tools": tools,
    })
}

/// Lit les arguments d'un appel : `Err` est la sortie d'échec de l'outil (`invalid_arguments`,
/// avec le champ en cause et un exemple), que le modèle lit pour se corriger.
pub fn parse(name: &str, arguments: Map<String, Value>) -> Result<ToolCall, ToolOutput> {
    let arguments = Value::Object(arguments);
    let read = match name {
        "search" => typed(arguments).and_then(|i: SearchInput| {
            check_search(&i)?;
            Ok(ToolCall::Search(i))
        }),
        "fetch" => typed(arguments).and_then(|i: FetchInput| {
            check_fetch(&i)?;
            Ok(ToolCall::Fetch(i))
        }),
        "create_pages" => typed(arguments).and_then(|i: CreatePagesInput| {
            check_create(&i)?;
            Ok(ToolCall::CreatePages(i))
        }),
        "update_page" => typed(arguments).and_then(|i: UpdatePageInput| {
            check_update(&i)?;
            Ok(ToolCall::UpdatePage(i))
        }),
        "manage_pages" => typed(arguments).and_then(|i: ManagePagesInput| {
            check_manage(&i)?;
            Ok(ToolCall::ManagePages(i))
        }),
        "workspace_info" => typed(arguments).and_then(|i: WorkspaceInfoInput| {
            length("space", i.space.as_deref(), 1, MAX_NAME_CHARS)?;
            Ok(ToolCall::WorkspaceInfo(i))
        }),
        other => Err(format!(
            "outil inconnu : « {} »",
            mementum_md::excerpt(other)
        )),
    };
    read.map_err(|detail| {
        let example = all()
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.example.clone());
        text::refused_arguments(name, &detail, example.as_ref())
    })
}

/// Les arguments lus en leur type ; l'erreur dit en français le champ en cause.
fn typed<T: serde::de::DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_path_to_error::deserialize(arguments).map_err(|e| {
        let path = e.path().to_string();
        describe(e.inner(), &path)
    })
}

/// Une erreur de serde, dite en français, sans recopier l'entrée (Ruling R434) : le texte de
/// serde a des tournures stables (`serde::de::Error`), dont on tire ce qui est en cause.
fn describe(error: &serde_json::Error, path: &str) -> String {
    let text = error.to_string();
    let at = if path.is_empty() || path == "." {
        String::new()
    } else {
        format!(" (« {path} »)")
    };
    let quoted = |prefix: &str| {
        text.strip_prefix(prefix)
            .and_then(|rest| rest.split_once('`'))
            .map(|(name, _)| mementum_md::excerpt(name))
    };
    // Ce que serde attendait suit la **dernière** tournure « , expected » : une valeur refusée
    // qui l'écrirait elle-même ne s'y glisse pas.
    let expected = text
        .rsplit_once(", expected ")
        .map(|(_, expected)| expected.split(" at line ").next().unwrap_or(expected));
    if let Some(name) = quoted("unknown field `") {
        format!("champ inconnu : « {name} »{at}")
    } else if let Some(name) = quoted("missing field `") {
        format!("champ manquant : « {name} »{at}")
    } else if let Some(name) = quoted("duplicate field `") {
        format!("champ en double : « {name} »{at}")
    } else if let Some(name) = quoted("unknown variant `") {
        let accepted = expected
            .map(|e| e.replace('`', "").replace("one of ", ""))
            .filter(|e| e.len() <= 400)
            .map(|e| format!(" ; attendu : {e}"))
            .unwrap_or_default();
        format!("valeur inconnue « {name} »{at}{accepted}")
    } else if text.starts_with("invalid type") {
        let wanted = expected.map(french_type).unwrap_or_default();
        format!("type invalide{at}{wanted}")
    } else if text.starts_with("invalid length") {
        format!("longueur invalide{at}")
    } else {
        format!("valeur invalide{at}")
    }
}

/// Le type que serde attendait, en français, quand on le reconnaît.
fn french_type(expected: &str) -> String {
    let wanted = match expected {
        "a string" => "un texte",
        "a boolean" => "un booléen",
        "a sequence" => "une liste",
        "u32" | "u64" | "usize" => "un entier positif",
        _ if expected.starts_with("struct ")
            || expected.starts_with("internally tagged enum")
            || expected == "a map" =>
        {
            "un objet"
        }
        _ if expected.starts_with("enum ") => "un des textes permis",
        _ => return String::new(),
    };
    format!(" : attendu {wanted}")
}

/// Un texte de `min` à `max` caractères ; absent, il passe.
fn length(field: &str, value: Option<&str>, min: usize, max: usize) -> Result<(), String> {
    let Some(value) = value else { return Ok(()) };
    // On ne compte pas plus loin que la borne : une entrée de 2 Mio s'arrête là.
    let count = value.chars().take(max + 1).count();
    if count < min {
        Err(if min == 1 {
            format!("« {field} » ne doit pas être vide")
        } else {
            format!("« {field} » : {min} caractères au moins")
        })
    } else if count > max {
        Err(format!("« {field} » : {max} caractères au plus"))
    } else {
        Ok(())
    }
}

/// Une liste de `min` à `max` éléments.
fn count(field: &str, len: usize, min: usize, max: usize) -> Result<(), String> {
    if len < min {
        Err(format!("« {field} » : {min} élément au moins"))
    } else if len > max {
        Err(format!(
            "« {field} » : {max} éléments au plus (reçu : {len})"
        ))
    } else {
        Ok(())
    }
}

fn check_search(input: &SearchInput) -> Result<(), String> {
    length("query", Some(&input.query), 1, MAX_QUERY_CHARS)?;
    length("space", input.space.as_deref(), 1, MAX_NAME_CHARS)?;
    length("created_by", input.created_by.as_deref(), 1, MAX_NAME_CHARS)?;
    length(
        "edited_after",
        input.edited_after.as_deref(),
        1,
        MAX_TOKEN_CHARS,
    )?;
    length(
        "edited_before",
        input.edited_before.as_deref(),
        1,
        MAX_TOKEN_CHARS,
    )?;
    length("after", input.after.as_deref(), 1, MAX_TOKEN_CHARS)
}

fn check_fetch(input: &FetchInput) -> Result<(), String> {
    length("id", Some(&input.id), 1, MAX_PAGE_REF_CHARS)?;
    length("from", input.from.as_deref(), 1, MAX_ANCHOR_CHARS)?;
    length("since", input.since.as_deref(), 1, MAX_TOKEN_CHARS)?;
    match input.limit {
        Some(limit) if limit == 0 || limit > MAX_LIMIT => Err(format!(
            "« limit » : de 1 à {MAX_LIMIT} octets (par défaut {DEFAULT_LIMIT})"
        )),
        _ => Ok(()),
    }
}

fn check_create(input: &CreatePagesInput) -> Result<(), String> {
    length("space", input.space.as_deref(), 1, MAX_NAME_CHARS)?;
    length("parent", input.parent.as_deref(), 1, MAX_PAGE_REF_CHARS)?;
    count("pages", input.pages.len(), 1, MAX_PAGES)?;
    let mut total = 0;
    for (i, page) in input.pages.iter().enumerate() {
        length(
            &format!("pages[{i}].title"),
            Some(&page.title),
            0,
            MAX_TITLE_CHARS,
        )?;
        let markdown = page.markdown.as_deref().unwrap_or("");
        length(
            &format!("pages[{i}].markdown"),
            Some(markdown),
            0,
            MAX_MARKDOWN_CHARS,
        )?;
        total += markdown.chars().count();
        if total > MAX_MARKDOWN_CHARS {
            return Err(format!(
                "les « markdown » des pages ensemble : {MAX_MARKDOWN_CHARS} caractères au plus ; \
                 crée les pages en plusieurs appels"
            ));
        }
    }
    Ok(())
}

/// Une place : un seul de `after`, `before`, `position`.
fn one_place(
    at: &str,
    after: &Option<String>,
    before: &Option<String>,
    position: &Option<Position>,
) -> Result<(), String> {
    let given = [after.is_some(), before.is_some(), position.is_some()];
    if given.iter().filter(|g| **g).count() != 1 {
        return Err(format!(
            "« {at} » : donne un seul de « after », « before » et « position »"
        ));
    }
    length(
        &format!("{at}.after"),
        after.as_deref(),
        1,
        MAX_ANCHOR_CHARS,
    )?;
    length(
        &format!("{at}.before"),
        before.as_deref(),
        1,
        MAX_ANCHOR_CHARS,
    )
}

fn check_update(input: &UpdatePageInput) -> Result<(), String> {
    length("page", Some(&input.page), 1, MAX_PAGE_REF_CHARS)?;
    length(
        "base_revision",
        input.base_revision.as_deref(),
        1,
        MAX_TOKEN_CHARS,
    )?;
    count("operations", input.operations.len(), 1, MAX_OPERATIONS)?;
    for (i, operation) in input.operations.iter().enumerate() {
        let at = format!("operations[{i}]");
        match operation {
            Operation::StrReplace { old, anchor, .. } => {
                length(&format!("{at}.old"), Some(old), 1, MAX_OLD_CHARS)?;
                length(
                    &format!("{at}.anchor"),
                    anchor.as_deref(),
                    1,
                    MAX_ANCHOR_CHARS,
                )?;
            }
            Operation::Insert {
                after,
                before,
                position,
                markdown,
            } => {
                one_place(&at, after, before, position)?;
                markdown_length(&at, markdown)?;
            }
            Operation::ReplaceBlock {
                anchor, markdown, ..
            } => {
                length(&format!("{at}.anchor"), Some(anchor), 1, MAX_ANCHOR_CHARS)?;
                markdown_length(&at, markdown)?;
            }
            Operation::DeleteBlock { anchor, .. } => {
                length(&format!("{at}.anchor"), Some(anchor), 1, MAX_ANCHOR_CHARS)?;
            }
            Operation::MoveBlock {
                anchor,
                after,
                before,
                position,
            } => {
                length(&format!("{at}.anchor"), Some(anchor), 1, MAX_ANCHOR_CHARS)?;
                one_place(&at, after, before, position)?;
            }
            Operation::ReplacePage { markdown, .. } => markdown_length(&at, markdown)?,
            Operation::SetProperties { properties } => {
                length(
                    &format!("{at}.properties.title"),
                    Some(&properties.title),
                    0,
                    MAX_TITLE_CHARS,
                )?;
            }
        }
    }
    Ok(())
}

/// Le MMD d'une opération : [`MAX_MARKDOWN_CHARS`] caractères au plus.
fn markdown_length(at: &str, markdown: &str) -> Result<(), String> {
    length(
        &format!("{at}.markdown"),
        Some(markdown),
        0,
        MAX_MARKDOWN_CHARS,
    )
}

fn check_manage(input: &ManagePagesInput) -> Result<(), String> {
    length("page", Some(&input.page), 1, MAX_PAGE_REF_CHARS)?;
    length("after", input.after.as_deref(), 1, MAX_PAGE_REF_CHARS)?;
    length("title", input.title.as_deref(), 0, MAX_TITLE_CHARS)?;
    match (input.action, &input.title, &input.after) {
        (PageAction::Rename, None, _) => Err("« rename » demande « title »".into()),
        (action, Some(_), _) if action != PageAction::Rename => {
            Err("« title » ne sert qu'à « rename »".into())
        }
        (action, _, Some(_)) if action != PageAction::Move => {
            Err("« after » ne sert qu'à « move »".into())
        }
        _ => Ok(()),
    }
}
