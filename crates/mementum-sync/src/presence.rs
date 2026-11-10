//! La forme d'un état de présence (Ruling R119).
//!
//! Le serveur ne relayait d'un état de présence que ce qu'il en vérifiait : que c'était du
//! JSON. Or le client en fait deux usages sans garde. La couleur d'un curseur est posée telle
//! quelle dans un attribut `style` (BlockNote, `setAttribute("style", …)`) : une couleur comme
//! `red;position:fixed;…` y est du CSS. Et la position d'un curseur est décodée à chaque
//! changement venu du réseau (y-prosemirror, `createDecorations`) : mal formée, elle lève une
//! exception, l'éditeur de qui la reçoit n'affiche plus ce qu'écrivent les autres, et la frappe
//! suivante de cette personne récrit le document d'après ce qu'elle voit, en effaçant ce
//! qu'ils avaient écrit entre-temps (vu dans un navigateur, e2e `presence-guard.spec.ts`).
//!
//! Un état n'est donc gardé et relayé que s'il a la forme de ceux que le client écrit :
//! - un objet dont les seules clés possibles sont `user` et `cursor`. L'objet vide est l'état
//!   initial de y-protocols ;
//! - `user` : un objet dont les seules clés possibles sont `name` (une chaîne de 1 à
//!   [`MAX_NAME_CHARS`] caractères, sans caractère de contrôle) et `color` (`#` suivi de six
//!   chiffres hexadécimaux, la seule forme que y-prosemirror tient pour une couleur) ;
//! - `cursor` : `null`, ou un objet aux clés `anchor` et `head`, toutes deux présentes (le
//!   client lit l'une et l'autre), chacune une position relative de Yjs
//!   (`relativePositionToJSON`) : un objet dont les seules clés possibles sont `type`, `tname`,
//!   `item` et `assoc`. `type` et `item` : `null` ou `{client, clock}`, deux entiers ; `tname` :
//!   `null` ou le nom du fragment de l'éditeur ; l'un des trois au moins n'est pas nul (Yjs
//!   lève une exception sinon) ; `assoc` : un entier de 32 bits.
//!
//! Ce qui est rendu n'est pas le texte reçu, mais l'état lu, réécrit : ce que les clients
//! relisent est exactement ce qui a été vérifié ici (Ruling R123).

use mementum_crdt::FRAGMENT;
use serde_json::{Map, Value};

use crate::protocol::MAX_CLIENT;

/// Taille maximale d'un état de présence (JSON, en octets) : au-delà, l'entrée est ignorée.
///
/// Un état honnête porte un nom de 100 caractères au plus, une couleur et deux positions de
/// curseur : quelques centaines d'octets, moins de 1 Kio avec un nom de 100 caractères de
/// quatre octets. Chaque connexion peut en tenir huit, et tous sont envoyés à qui arrive : à
/// 64 Kio l'état, la borne d'avant, 24 connexions d'un compte faisaient un accueil de 12 Mio.
pub(crate) const MAX_PRESENCE_STATE: usize = 2 * 1024;

/// Longueur maximale du nom annoncé, en caractères : celle d'un nom de compte.
const MAX_NAME_CHARS: usize = 100;

/// L'état de présence `json`, réécrit, s'il a la forme connue ; `None` sinon : trop gros,
/// illisible, ou d'une autre forme. L'état `null` (le retrait d'un état) n'en est pas un :
/// l'appelant le traite à part.
pub(crate) fn known_shape(json: &str) -> Option<String> {
    if json.len() > MAX_PRESENCE_STATE {
        return None;
    }
    let state: Value = serde_json::from_str(json).ok()?;
    let fields = only_keys(&state, &["user", "cursor"])?;
    let user = fields.get("user").is_none_or(is_user);
    let cursor = fields.get("cursor").is_none_or(is_cursor);
    (user && cursor).then(|| state.to_string())
}

/// L'objet `value`, s'il en est un et n'a pas d'autre clé que `allowed`.
fn only_keys<'a>(value: &'a Value, allowed: &[&str]) -> Option<&'a Map<String, Value>> {
    let object = value.as_object()?;
    object
        .keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(object)
}

fn is_user(value: &Value) -> bool {
    let Some(user) = only_keys(value, &["name", "color"]) else {
        return false;
    };
    user.get("name").is_none_or(is_name) && user.get("color").is_none_or(is_colour)
}

fn is_name(value: &Value) -> bool {
    value.as_str().is_some_and(|name| {
        (1..=MAX_NAME_CHARS).contains(&name.chars().count()) && !name.chars().any(char::is_control)
    })
}

/// `#` et six chiffres hexadécimaux : `^#[0-9a-fA-F]{6}$`, comme y-prosemirror.
fn is_colour(value: &Value) -> bool {
    value.as_str().is_some_and(|colour| {
        colour.len() == 7
            && colour.starts_with('#')
            && colour[1..].bytes().all(|b| b.is_ascii_hexdigit())
    })
}

fn is_cursor(value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(cursor) = only_keys(value, &["anchor", "head"]) else {
        return false;
    };
    // Les deux : le client lit `anchor` puis `head`, sans regarder s'ils sont là.
    ["anchor", "head"]
        .iter()
        .all(|end| cursor.get(*end).is_some_and(is_relative_position))
}

fn is_relative_position(value: &Value) -> bool {
    let Some(position) = only_keys(value, &["type", "tname", "item", "assoc"]) else {
        return false;
    };
    let present = |key: &str| position.get(key).filter(|value| !value.is_null());
    let (kind, tname, item) = (present("type"), present("tname"), present("item"));
    // Yjs ne sait rien faire d'une position qui ne nomme ni élément, ni type : il lève une
    // exception (`createAbsolutePositionFromRelativePosition`, `unexpectedCase`).
    if kind.is_none() && tname.is_none() && item.is_none() {
        return false;
    }
    kind.is_none_or(is_id)
        && item.is_none_or(is_id)
        && tname.is_none_or(|name| name.as_str() == Some(FRAGMENT))
        && position.get("assoc").is_none_or(is_assoc)
}

/// Un identifiant de Yjs : `{client, clock}`, sans autre clé.
fn is_id(value: &Value) -> bool {
    let Some(id) = only_keys(value, &["client", "clock"]) else {
        return false;
    };
    let below = |key: &str, max: u64| {
        id.get(key)
            .and_then(Value::as_u64)
            .is_some_and(|n| n <= max)
    };
    below("client", MAX_CLIENT) && below("clock", u64::from(u32::MAX))
}

fn is_assoc(value: &Value) -> bool {
    value
        .as_i64()
        .is_some_and(|assoc| i32::try_from(assoc).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une position relative de Yjs, à la fin du fragment de l'éditeur.
    const END: &str = r#"{"tname":"document-store","assoc":0}"#;
    /// Une position dans un bloc : ce que le client écrit quand le curseur est dans du texte.
    const IN_TEXT: &str = r#"{"type":{"client":1440742776,"clock":6},"item":{"client":206716952,"clock":41},"assoc":-1}"#;

    fn cursor(anchor: &str, head: &str) -> String {
        format!(
            r##"{{"user":{{"name":"Mona","color":"#B4462B"}},"cursor":{{"anchor":{anchor},"head":{head}}}}}"##
        )
    }

    /// L'état relu, comme le relit un client.
    fn parsed(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_shapes_the_client_writes_are_known() {
        let long_name = format!(r#"{{"user":{{"name":"{}"}}}}"#, "é".repeat(100));
        let top_ids = format!(
            r#"{{"type":{{"client":{},"clock":{}}},"assoc":{}}}"#,
            MAX_CLIENT,
            u32::MAX,
            i32::MIN
        );
        for state in [
            // L'état initial de y-protocols, puis ce que BlockNote y pose.
            "{}".to_string(),
            r##"{"user":{"name":"Mona","color":"#B4462B"}}"##.to_string(),
            r##"{"user":{"name":"Mona","color":"#b4462b"},"cursor":null}"##.to_string(),
            cursor(END, END),
            cursor(IN_TEXT, END),
            // Ce que le client tolère aussi : il a des valeurs par défaut pour le nom et la
            // couleur, et lit `null` comme une absence.
            r#"{"user":{}}"#.to_string(),
            r#"{"user":{"name":"Mona"}}"#.to_string(),
            r##"{"user":{"color":"#000000"}}"##.to_string(),
            r#"{"cursor":null}"#.to_string(),
            cursor(r#"{"type":null,"tname":"document-store","item":null}"#, END),
            cursor(r#"{"item":{"client":0,"clock":0}}"#, END),
            cursor(&top_ids, END),
            long_name,
        ] {
            let kept = known_shape(&state);
            assert!(kept.is_some(), "refusé : {state}");
            // Réécrit, il dit la même chose.
            assert_eq!(parsed(&kept.unwrap()), parsed(&state), "{state}");
        }
    }

    #[test]
    fn other_shapes_are_not() {
        let name = |name: &str| format!(r#"{{"user":{{"name":{name}}}}}"#);
        let colour = |colour: &str| format!(r#"{{"user":{{"name":"Mona","color":{colour}}}}}"#);
        let too_long = format!(r#""{}""#, "a".repeat(101));
        let over_client = format!(r#"{{"type":{{"client":{},"clock":0}}}}"#, MAX_CLIENT + 1);
        let over_clock = format!(
            r#"{{"type":{{"client":1,"clock":{}}}}}"#,
            u64::from(u32::MAX) + 1
        );
        let over_assoc = format!(
            r#"{{"tname":"document-store","assoc":{}}}"#,
            i64::from(i32::MAX) + 1
        );
        let cases: Vec<(&str, String)> = vec![
            ("pas un objet", "[]".into()),
            ("pas un objet", r#""user""#.into()),
            ("pas un objet", "7".into()),
            ("pas du JSON", "{".into()),
            ("clé inconnue", r#"{"surprise":true}"#.into()),
            (
                "clé inconnue à côté d'un état en règle",
                r#"{"user":{"name":"Mona"},"x":1}"#.into(),
            ),
            ("user nul", r#"{"user":null}"#.into()),
            ("user qui n'est pas un objet", r#"{"user":"Mona"}"#.into()),
            ("user qui n'est pas un objet", r#"{"user":7}"#.into()),
            (
                "clé inconnue dans user",
                r#"{"user":{"name":"Mona","avatar":"x"}}"#.into(),
            ),
            ("nom vide", name(r#""""#)),
            ("nom trop long", name(&too_long)),
            ("nom qui n'est pas une chaîne", name("7")),
            ("nom nul", name("null")),
            ("nom avec un caractère de contrôle", name(r#""Mo\u0000na""#)),
            ("nom avec un saut de ligne", name(r#""Mo\nna""#)),
            (
                "couleur piégée",
                colour(
                    r#""red;position:fixed;inset:0;z-index:99999;background:url(https://evil.example/x.png)""#,
                ),
            ),
            ("couleur nommée", colour(r#""red""#)),
            ("couleur à trois chiffres", colour(r##""#f00""##)),
            ("couleur à huit chiffres", colour(r##""#ff000000""##)),
            ("couleur avec une suite", colour(r##""#ff0000;""##)),
            ("couleur hors hexadécimal", colour(r##""#ff00zz""##)),
            ("couleur sans dièse", colour(r#""ff0000f""#)),
            ("couleur qui n'est pas une chaîne", colour("42")),
            ("couleur nulle", colour("null")),
            ("curseur qui n'est pas un objet", r#"{"cursor":7}"#.into()),
            ("curseur vide", r#"{"cursor":{}}"#.into()),
            (
                "curseur sans head",
                format!(r#"{{"cursor":{{"anchor":{END}}}}}"#),
            ),
            (
                "curseur sans anchor",
                format!(r#"{{"cursor":{{"head":{END}}}}}"#),
            ),
            (
                "clé inconnue dans le curseur",
                format!(r#"{{"cursor":{{"anchor":{END},"head":{END},"x":1}}}}"#),
            ),
            ("anchor qui n'est pas un objet", cursor("7", "7")),
            ("head qui n'est pas un objet", cursor(END, "7")),
            ("anchor nul", cursor("null", END)),
            ("position sans rien", cursor("{}", END)),
            (
                "position aux trois champs nuls",
                cursor(r#"{"type":null,"tname":null,"item":null}"#, END),
            ),
            ("position avec assoc seul", cursor(r#"{"assoc":0}"#, END)),
            (
                "clé inconnue dans une position",
                cursor(r#"{"tname":"document-store","x":1}"#, END),
            ),
            (
                "tname d'un autre fragment",
                cursor(r#"{"tname":"autre"}"#, END),
            ),
            (
                "tname qui n'est pas une chaîne",
                cursor(r#"{"tname":7}"#, END),
            ),
            ("type qui n'est pas un objet", cursor(r#"{"type":7}"#, END)),
            ("type sans clock", cursor(r#"{"type":{"client":1}}"#, END)),
            (
                "clé inconnue dans un identifiant",
                cursor(r#"{"type":{"client":1,"clock":1,"x":1}}"#, END),
            ),
            (
                "client négatif",
                cursor(r#"{"type":{"client":-1,"clock":1}}"#, END),
            ),
            (
                "client à virgule",
                cursor(r#"{"type":{"client":1.5,"clock":1}}"#, END),
            ),
            (
                "client écrit en flottant",
                cursor(r#"{"type":{"client":1e3,"clock":1}}"#, END),
            ),
            (
                "client en chaîne",
                cursor(r#"{"type":{"client":"1","clock":1}}"#, END),
            ),
            ("client hors de 53 bits", cursor(&over_client, END)),
            ("clock hors de 32 bits", cursor(&over_clock, END)),
            (
                "item qui n'est pas un objet",
                cursor(r#"{"item":[1,2]}"#, END),
            ),
            ("assoc hors de 32 bits", cursor(&over_assoc, END)),
            (
                "assoc à virgule",
                cursor(r#"{"tname":"document-store","assoc":0.5}"#, END),
            ),
            (
                "assoc nul",
                cursor(r#"{"tname":"document-store","assoc":null}"#, END),
            ),
            (
                "assoc en chaîne",
                cursor(r#"{"tname":"document-store","assoc":"0"}"#, END),
            ),
        ];
        for (what, state) in cases {
            assert_eq!(known_shape(&state), None, "{what} : {state}");
        }
    }

    /// Une clé répétée : la dernière fait foi, pour ce vérificateur comme pour `JSON.parse`, et
    /// l'état réécrit ne la porte qu'une fois. Ce que le client relit est ce qui a été vérifié.
    #[test]
    fn rewritten_state_is_what_was_checked() {
        let twice = r##"{"user":{"name":"Mona","color":"#B4462B"},"user":{"name":"Mona","color":"red;x"}}"##;
        assert_eq!(known_shape(twice), None);
        let twice = r##"{"user":{"name":"Mona","color":"red;x"},"user":{"name":"Mona","color":"#B4462B"}}"##;
        let kept = known_shape(twice).expect("la dernière valeur est en règle");
        assert_eq!(kept.matches("color").count(), 1, "{kept}");
        assert!(!kept.contains("red"), "{kept}");
        // Des espaces, un échappement : réécrits sans.
        let spaced = "{ \"user\" : { \"n\\u0061me\" : \"Mona\" } }";
        assert_eq!(
            known_shape(spaced).as_deref(),
            Some(r#"{"user":{"name":"Mona"}}"#)
        );
    }

    #[test]
    fn a_state_over_the_size_limit_is_not() {
        let state =
            |name_bytes: usize| format!(r#"{{"user":{{"name":"{}"}}}}"#, "a".repeat(name_bytes));
        // Un nom de 100 caractères de quatre octets tient largement.
        let widest = format!(r#"{{"user":{{"name":"{}"}}}}"#, "🚀".repeat(100));
        assert!(widest.len() < MAX_PRESENCE_STATE);
        assert!(known_shape(&widest).is_some());
        // La taille est celle du texte reçu, regardée avant toute lecture.
        let padded = |total: usize| {
            let base = state(100);
            format!("{base}{}", " ".repeat(total - base.len()))
        };
        assert_eq!(MAX_PRESENCE_STATE, 2048);
        assert!(known_shape(&padded(2048)).is_some());
        assert_eq!(known_shape(&padded(2049)), None);
    }
}
