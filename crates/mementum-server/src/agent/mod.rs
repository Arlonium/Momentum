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

/// Le refus d'un MMD d'agent qui commence par l'en-tête de `fetch` ([`fetch_header`]) : un message
/// que le serveur écrit seul (il ne cite rien de l'entrée), qui dit quoi retirer.
pub(crate) const FETCH_HEADER_REFUSAL: &str = "le MMD commence par l'en-tête que fetch met avant \
     le contenu d'une page (de « titre : » jusqu'à la ligne « --- ») : il n'est pas du MMD ; \
     retire-le, et n'envoie que ce qui suit la ligne « --- » (le titre d'une page se donne à part)";

/// Le plus de lignes que l'en-tête de `fetch` porte avant `---` : le titre, le chemin, « sous-pages : »
/// et ses lignes (`MAX_CHILDREN` de `mementum_mcp::text`, et la ligne qui dit combien il en tait),
/// avec de la marge.
const FETCH_HEADER_LINES: usize = 256;

/// Le MMD d'un agent commence-t-il par l'en-tête que `fetch` écrit avant le contenu d'une page
/// (`titre : …`, puis `chemin :`, `sous-pages :` et ses lignes, jusqu'à `---`), recopié avec lui
/// (recette du lot 5, D6, Ruling R1550) ? Une première ligne qui commence par `titre : `, puis une
/// ligne `---` avant toute ligne vide. Sans ligne vide entre elles, ces lignes ne sont jamais du MMD
/// canonique (un titre souligné, ou des lignes qui se suivent sans ligne vide) : rien de ce que le
/// serveur accepterait n'est pris pour l'en-tête (`no_header_shape_is_canonical_mmd`). Le refus
/// qui le nomme remplace celui de la lecture, qui parlait d'un « titre souligné » et menait à faire
/// du titre de la page un titre du contenu.
pub(crate) fn fetch_header(markdown: &str) -> bool {
    let mut lines = markdown.lines();
    if !lines
        .next()
        .is_some_and(|first| first.starts_with("titre : "))
    {
        return false;
    }
    for line in lines.take(FETCH_HEADER_LINES) {
        if line.trim().is_empty() {
            return false;
        }
        if line.trim_end() == "---" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ce que [`fetch_header`] reconnaît n'est jamais du MMD canonique : le refus qui nomme l'en-tête
    /// ne prend la place que d'un refus. Les formes de l'en-tête que `fetch` écrit, et ce qu'un agent
    /// en fait en le recopiant (avec ou sans le contenu, une fin de ligne Windows).
    #[test]
    fn no_header_shape_is_canonical_mmd() {
        let shapes = [
            "titre : Notes\n---\n",
            "titre : Notes\n---\nUn paragraphe.\n",
            "titre : Notes\n---\n\nUn paragraphe.\n",
            "titre : Notes\nchemin : Projet / Sous\n---\nUn paragraphe.\n",
            "titre : Notes\nsous-pages :\n- Détails (01a1ffa0-9210-76dd-b3dd-05b56f0a2f82)\n---\nUn.\n",
            "titre : Notes\nsous-pages :\n- A (x)\n- B (y)\net 3 autres\n---\n",
            "titre : Notes\r\n---\r\nUn paragraphe.\r\n",
            "titre : ## Titre\n---\n",
        ];
        for shape in shapes {
            assert!(fetch_header(shape), "{shape:?}");
            assert!(mementum_md::parse(shape).is_err(), "{shape:?} se lit");
        }
        // Ce qui n'est pas l'en-tête : une ligne vide avant `---`, pas de `---`, autre chose en tête.
        for other in [
            "titre : Notes\n\n---\n",
            "titre : Notes\n",
            "Un paragraphe.\n\n---\n",
            "titre: Notes\n---\n",
            " titre : Notes\n---\n",
            "",
        ] {
            assert!(!fetch_header(other), "{other:?}");
        }
    }
}
