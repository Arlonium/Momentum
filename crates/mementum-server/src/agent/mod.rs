//! Ce que partagent les routes REST et les outils MCP d'un agent (lot 5) : l'identité par jeton
//! d'API, qui restreint les droits de son compte ([`grant`]), et le débit et le journal
//! d'activité de ses appels (`calls`). Les opérations des agents viendront ici (Task S7).
//!
//! **Un jeton ne crée pas un second chemin de droits** (Rulings R1216, R1217) : un appel par
//! jeton est un appel du compte du jeton, restreint dans `access.rs`, au même endroit que tout
//! calcul de droits ; aucune route ne lit la table des jetons pour juger un droit, et aucune ne
//! lit `AuthUser::via` (un test lit les sources : ce nom ne paraît que dans `auth.rs`,
//! `access.rs` et ce module).

pub mod calls;
pub mod grant;
