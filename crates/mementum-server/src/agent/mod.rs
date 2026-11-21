//! Ce que partagent les routes REST et les outils MCP d'un agent (lot 5) : l'identité par jeton
//! d'API, qui restreint les droits de son compte ([`grant`]), et le débit et le journal
//! d'activité de ses appels (`calls`) ; les opérations des agents (Task S7), que les routes REST
//! et les outils MCP (Task M2) appellent : lire une page (`fetch`), l'écrire par opérations
//! (`update`), créer des pages en lot (`create`), chercher (`search`), gérer une page (`manage`),
//! le compte et ses espaces (`info`), et les révisions servies (`revisions`). L'outil MCP de
//! chaque opération (`routes/mcp.rs`) ne fait que lire ses arguments, mener la fonction d'ici, et
//! en mettre la réponse en texte : aucun outil n'a de droit à lui.
//!
//! **Un jeton ne crée pas un second chemin de droits** (Rulings R1216, R1217) : un appel par
//! jeton est un appel du compte du jeton, restreint dans `access.rs`, au même endroit que tout
//! calcul de droits ; aucune route ne lit la table des jetons pour juger un droit, et aucune ne
//! lit `AuthUser::via` (un test lit les sources : ce nom ne paraît que dans `auth.rs`,
//! `access.rs` et ce module).

pub mod calls;
pub(crate) mod create;
pub(crate) mod fetch;
pub mod grant;
pub(crate) mod info;
pub(crate) mod manage;
pub mod revisions;
pub(crate) mod search;
pub(crate) mod update;

/// Le MMD d'un agent, une fin de ligne ajoutée s'il n'en finit pas par une (correction groupée,
/// défaut D2 de la passe de fin, Ruling R1521) : le format veut que chaque ligne finisse par
/// `\n`, et le vrai Claude Code l'oubliait à chaque `insert` ou `replace_block`, d'où un appel
/// perdu et un refus au journal d'activité. Pour les entrées des agents seulement (outils MCP et
/// routes des agents : opérations, lot de pages) ; `PUT /mmd`, la route d'une session, garde la
/// règle du format. Un MMD vide reste vide.
pub(crate) fn agent_mmd(markdown: &str) -> std::borrow::Cow<'_, str> {
    if markdown.is_empty() || markdown.ends_with('\n') {
        std::borrow::Cow::Borrowed(markdown)
    } else {
        std::borrow::Cow::Owned(format!("{markdown}\n"))
    }
}
