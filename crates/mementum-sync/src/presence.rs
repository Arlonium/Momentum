//! La forme d'un état de présence (Ruling R119), son nom (Ruling R136), et à quel compte
//! appartient chaque client d'awareness ([`Holders`], Rulings R101, R136).
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
//!
//! Pour une connexion qui a un compte (le nom que la route de synchro lit avec la session), le
//! nom n'est pas jugé mais remplacé par celui du compte, tel que le serveur le montre
//! ([`display_name`], Ruling R136) : un client n'annonce plus le nom qu'il veut (R52), et un nom
//! de compte à caractère de contrôle ne prive plus son compte de présence (conséquence de R119).

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use mementum_crdt::FRAGMENT;
use serde_json::{Map, Value};
use uuid::Uuid;
use yrs::ClientID;

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

/// Le nom montré pour un compte qui n'en a pas qu'on puisse montrer ([`display_name`]).
const NO_NAME: &str = "Sans nom";

/// Le nom montré pour le compte qui s'appelle `name` (Ruling R136) : sans ses caractères de
/// contrôle, coupé à [`MAX_NAME_CHARS`] caractères, « Sans nom » s'il n'en reste rien.
///
/// Le serveur l'écrit dans la liste des présents (trame `roster`) et dans l'état de présence de
/// chaque connexion qui a un compte. Il a donc toujours la forme d'un nom que la garde du client
/// accepte (`apps/web/src/editor/presenceGuard.ts`) : de 1 à 100 caractères, comptés comme elle
/// les compte, par point de code (`[...name].length`), pas par octet ni par unité UTF-16 ; aucun
/// caractère de contrôle, la catégorie Cc pour l'une comme pour l'autre (`char::is_control` ici,
/// `\p{Cc}` là-bas). Un nom de compte est déjà borné à l'inscription, mais pas ses caractères
/// (Ruling R119 : un nom à caractère de contrôle privait son compte de présence), et un compte
/// créé autrement que par l'inscription n'y passe pas.
pub fn display_name(name: &str) -> String {
    let shown: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    if shown.is_empty() {
        NO_NAME.to_string()
    } else {
        shown
    }
}

/// L'état de présence `json`, réécrit, s'il a la forme connue ; `None` sinon : trop gros,
/// illisible, ou d'une autre forme. L'état `null` (le retrait d'un état) n'en est pas un :
/// l'appelant le traite à part.
///
/// `account` : le nom que le serveur montre pour le compte de la connexion ([`display_name`]),
/// s'il en a un (Ruling R136). Il remplace le nom de l'état, quel qu'il soit : absent, mal formé
/// ou mensonger, le nom annoncé n'est pas jugé, et un état sans `user` en reçoit un. Le reste
/// est jugé comme d'habitude (`user`, s'il est là, reste un objet aux seules clés `name` et
/// `color`), et gardé tel que le client l'a écrit, à sa place. Un nom de compte peut faire
/// dépasser [`MAX_PRESENCE_STATE`] à l'état réécrit, de 400 octets au plus (cent caractères de
/// quatre octets) : la borne est celle du texte reçu. Sans compte (`None`, les tests du hub), le
/// nom annoncé est jugé, et gardé.
pub(crate) fn known_shape(json: &str, account: Option<&str>) -> Option<String> {
    if json.len() > MAX_PRESENCE_STATE {
        return None;
    }
    let mut state: Value = serde_json::from_str(json).ok()?;
    let fields = only_keys(&state, &["user", "cursor"])?;
    let user = fields
        .get("user")
        .is_none_or(|user| is_user(user, account.is_none()));
    let cursor = fields.get("cursor").is_none_or(is_cursor);
    if !(user && cursor) {
        return None;
    }
    if let Some(name) = account {
        let user = state
            .as_object_mut()?
            .entry("user")
            .or_insert_with(|| Value::Object(Map::new()));
        user.as_object_mut()?
            .insert("name".to_string(), Value::String(name.to_string()));
    }
    Some(state.to_string())
}

/// L'objet `value`, s'il en est un et n'a pas d'autre clé que `allowed`.
fn only_keys<'a>(value: &'a Value, allowed: &[&str]) -> Option<&'a Map<String, Value>> {
    let object = value.as_object()?;
    object
        .keys()
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(object)
}

/// `user` : un objet aux seules clés `name` et `color`, celle-ci en règle ; `name` aussi, si
/// `judge_name` (sans compte : sinon le nom annoncé est remplacé, et il n'y a rien à juger).
fn is_user(value: &Value, judge_name: bool) -> bool {
    let Some(user) = only_keys(value, &["name", "color"]) else {
        return false;
    };
    (!judge_name || user.get("name").is_none_or(is_name)) && user.get("color").is_none_or(is_colour)
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

/// Clients d'awareness qu'une page vivante retient pour leur compte sans qu'aucune connexion
/// vivante les tienne, au plus (Ruling R136, [`Holders`]).
///
/// Un onglet en laisse un à chaque départ (chaque ouverture de page tire un nouveau `ClientID`) :
/// quatre mille départs avant le premier oubli, sur une page que l'acteur garde ouverte tant que
/// quelqu'un y est. Une quarantaine d'octets chacun.
pub const MAX_REMEMBERED_CLIENTS: usize = 4096;

/// À quel compte appartient chaque client d'awareness de la page (Rulings R101, R136).
///
/// Un client appartient au compte de la première connexion qui en annonce un état gardé ; les
/// autres connexions du même compte peuvent le tenir avec elle. Quand la dernière part, il reste
/// au compte, sans connexion vivante, tant que l'acteur de la page vit : personne d'autre ne le
/// prend pendant une coupure, et la personne qui revient avec le même document le retrouve
/// (Ruling R109, cas a et c, fermés tant que l'acteur vit). La table part avec l'acteur : si la
/// page reste vide plus de `idle_close`, un compte qui a vu le client d'une personne peut ouvrir
/// la page le premier et le prendre, et elle reste invisible à son retour tant que l'acteur neuf
/// vit (test `after_the_actor_stops_another_account_can_take_a_departed_client`, registre du lot
/// 3, Task 6). Le lot 2 le libérait au départ : un autre compte qui
/// écrivait dans la page pouvait alors le prendre, ou avancer son horloge, et rendre invisible la
/// personne qui revenait. R109 l'acceptait parce que garder le compte d'un client aurait cassé un
/// changement de compte dans le même onglet sans rechargement ; depuis le Ruling R115, l'onglet
/// se recharge quand son compte change, et ce cas n'existe plus.
///
/// La table est bornée ([`MAX_REMEMBERED_CLIENTS`] clients retenus sans connexion vivante) : un
/// compte qui écrit dans la page peut la remplir de clients inventés, huit par connexion, en
/// ouvrant et fermant des connexions. Au-delà de la borne, elle oublie d'abord les clients du
/// compte qui en retient le plus, le plus ancien départ d'abord : qui la remplit seul n'y fait
/// oublier que les siens. Un client oublié retombe sur la règle du lot 2 (un autre compte le
/// prend par une entrée plus récente que ce que le serveur en sait, Ruling R112). Un client
/// tenu par une connexion vivante n'est jamais oublié, et ne compte pas dans la borne : il y en
/// a au plus huit par connexion. Remplir la table ne prive donc personne de présence (un client
/// neuf se prend toujours) et ne ferme aucune connexion.
#[derive(Default)]
pub(crate) struct Holders {
    clients: HashMap<ClientID, Holder>,
    /// Les clients qu'aucune connexion vivante ne tient, par compte, dans l'ordre de leur départ
    /// (la clé : son numéro). Un compte sans client retenu n'y figure pas.
    remembered: HashMap<Option<Uuid>, BTreeMap<u64, ClientID>>,
    /// Combien de clients y sont, tous comptes ensemble.
    remembered_count: usize,
    /// Départs comptés depuis le lancement de l'acteur : l'ordre dans lequel les clients ont été
    /// lâchés.
    departures: u64,
}

/// Ce que la table dit d'un client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Holding {
    /// Le compte qui le tient, ou l'a tenu ; `None` : des connexions sans compte.
    pub(crate) account: Option<Uuid>,
    /// Une connexion vivante le tient.
    pub(crate) live: bool,
}

struct Holder {
    account: Option<Uuid>,
    /// Les connexions vivantes qui le tiennent ; 0 : retenu, depuis le départ `left`.
    live: usize,
    left: u64,
}

impl Holders {
    /// À qui est `client`, s'il est à quelqu'un.
    pub(crate) fn get(&self, client: ClientID) -> Option<Holding> {
        self.clients.get(&client).map(|holder| Holding {
            account: holder.account,
            live: holder.live > 0,
        })
    }

    /// Une connexion du compte `account` prend `client`, qu'elle ne tenait pas : il était à
    /// personne, retenu pour ce compte, ou tenu par une autre connexion de ce compte. C'est
    /// `presence_rule` qui en décide : jamais le client d'un autre compte.
    pub(crate) fn take(&mut self, client: ClientID, account: Option<Uuid>) {
        match self.clients.get_mut(&client) {
            Some(holder) => {
                debug_assert_eq!(holder.account, account, "client pris à un autre compte");
                if holder.live == 0 {
                    // Retenu jusqu'ici : il ne l'est plus.
                    if let Some(kept) = self.remembered.get_mut(&holder.account) {
                        kept.remove(&holder.left);
                        if kept.is_empty() {
                            self.remembered.remove(&holder.account);
                        }
                    }
                    self.remembered_count -= 1;
                }
                holder.live += 1;
            }
            None => {
                self.clients.insert(
                    client,
                    Holder {
                        account,
                        live: 1,
                        left: 0,
                    },
                );
            }
        }
    }

    /// Une connexion qui tenait `client` part. `true` : c'était la dernière ; le client reste à
    /// son compte, retenu sans connexion vivante, et son état est à retirer. Appeler ensuite
    /// [`Holders::forget_beyond_bound`].
    pub(crate) fn release(&mut self, client: ClientID) -> bool {
        let Some(holder) = self.clients.get_mut(&client).filter(|h| h.live > 0) else {
            return false;
        };
        holder.live -= 1;
        if holder.live > 0 {
            return false;
        }
        holder.left = self.departures;
        self.departures += 1;
        self.remembered
            .entry(holder.account)
            .or_default()
            .insert(holder.left, client);
        self.remembered_count += 1;
        true
    }

    /// Oublie des clients retenus tant qu'il y en a plus que [`MAX_REMEMBERED_CLIENTS`] :
    /// d'abord ceux du compte qui en retient le plus, le plus ancien départ d'abord ; entre
    /// comptes à égalité, le plus ancien départ.
    ///
    /// Chaque oubli parcourt les comptes qui retiennent des clients : il n'en arrive qu'au
    /// départ d'une connexion, au plus huit (les clients qu'elle tenait), et jamais à une frappe
    /// ni à un changement de présence.
    pub(crate) fn forget_beyond_bound(&mut self) {
        while self.remembered_count > MAX_REMEMBERED_CLIENTS {
            let most = self
                .remembered
                .iter()
                .filter_map(|(account, kept)| {
                    let (oldest, _) = kept.first_key_value()?;
                    Some(((kept.len(), Reverse(*oldest)), *account))
                })
                .max()
                .map(|(_, account)| account);
            let Some(account) = most else {
                return;
            };
            let Some(kept) = self.remembered.get_mut(&account) else {
                return;
            };
            if let Some((_, client)) = kept.pop_first() {
                self.clients.remove(&client);
                self.remembered_count -= 1;
            }
            if kept.is_empty() {
                self.remembered.remove(&account);
            }
        }
    }

    /// Combien de clients sont retenus sans connexion vivante.
    #[cfg(test)]
    pub(crate) fn remembered(&self) -> usize {
        self.remembered_count
    }
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
            let kept = known_shape(&state, None);
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
            assert_eq!(known_shape(&state, None), None, "{what} : {state}");
        }
    }

    /// Une clé répétée : la dernière fait foi, pour ce vérificateur comme pour `JSON.parse`, et
    /// l'état réécrit ne la porte qu'une fois. Ce que le client relit est ce qui a été vérifié.
    #[test]
    fn rewritten_state_is_what_was_checked() {
        let twice = r##"{"user":{"name":"Mona","color":"#B4462B"},"user":{"name":"Mona","color":"red;x"}}"##;
        assert_eq!(known_shape(twice, None), None);
        let twice = r##"{"user":{"name":"Mona","color":"red;x"},"user":{"name":"Mona","color":"#B4462B"}}"##;
        let kept = known_shape(twice, None).expect("la dernière valeur est en règle");
        assert_eq!(kept.matches("color").count(), 1, "{kept}");
        assert!(!kept.contains("red"), "{kept}");
        // Des espaces, un échappement : réécrits sans.
        let spaced = "{ \"user\" : { \"n\\u0061me\" : \"Mona\" } }";
        assert_eq!(
            known_shape(spaced, None).as_deref(),
            Some(r#"{"user":{"name":"Mona"}}"#)
        );
    }

    /// Ruling R136 : le nom montré d'un compte a toujours la forme d'un nom que le vérificateur
    /// (et la garde du client, qui suit la même règle) accepte, quel que soit le nom du compte.
    #[test]
    fn display_name_always_has_the_shape_of_a_name() {
        let long = "é".repeat(150);
        let rockets = "🚀".repeat(101);
        let cases: Vec<(&str, String)> = vec![
            ("Mona", "Mona".into()),
            ("Zoé Lefèvre", "Zoé Lefèvre".into()),
            // Les caractères de contrôle partent, où qu'ils soient ; le reste est gardé tel quel.
            ("Mo\u{0}na\n", "Mona".into()),
            ("\u{7}\u{1b}[31mMona\u{7f}\u{85}", "[31mMona".into()),
            ("  Mona  ", "  Mona  ".into()),
            // Coupé à cent points de code, pas à cent octets.
            (&long, "é".repeat(100)),
            (&rockets, "🚀".repeat(100)),
            // Rien à montrer : « Sans nom ».
            ("", "Sans nom".into()),
            ("\u{0}\u{1}\n\t\u{9f}", "Sans nom".into()),
        ];
        for (account, expected) in cases {
            let shown = display_name(account);
            assert_eq!(shown, expected, "{account:?}");
            let state = serde_json::json!({ "user": { "name": shown } }).to_string();
            assert!(known_shape(&state, None).is_some(), "{account:?} : {state}");
        }
    }

    #[test]
    fn a_state_over_the_size_limit_is_not() {
        let state =
            |name_bytes: usize| format!(r#"{{"user":{{"name":"{}"}}}}"#, "a".repeat(name_bytes));
        // Un nom de 100 caractères de quatre octets tient largement.
        let widest = format!(r#"{{"user":{{"name":"{}"}}}}"#, "🚀".repeat(100));
        assert!(widest.len() < MAX_PRESENCE_STATE);
        assert!(known_shape(&widest, None).is_some());
        // La taille est celle du texte reçu, regardée avant toute lecture.
        let padded = |total: usize| {
            let base = state(100);
            format!("{base}{}", " ".repeat(total - base.len()))
        };
        assert_eq!(MAX_PRESENCE_STATE, 2048);
        assert!(known_shape(&padded(2048), None).is_some());
        assert_eq!(known_shape(&padded(2049), None), None);
    }

    /// Ruling R136 : pour une connexion qui a un compte, le nom de l'état est celui du compte.
    /// Annoncé, absent, mal formé ou mensonger, le nom du client est remplacé, et un état sans
    /// `user` en reçoit un ; le reste est gardé tel que le client l'a écrit, à sa place.
    #[test]
    fn the_account_name_replaces_the_announced_one() {
        let account = Some("Mallory Martin");
        let in_text = cursor(IN_TEXT, END);
        let with_cursor = parsed(&in_text);
        let cases: Vec<(String, String)> =
            vec![
            (
                r#"{"user":{"name":"Alice"}}"#.into(),
                r#"{"user":{"name":"Mallory Martin"}}"#.into(),
            ),
            ("{}".into(), r#"{"user":{"name":"Mallory Martin"}}"#.into()),
            (
                r#"{"cursor":null}"#.into(),
                r#"{"cursor":null,"user":{"name":"Mallory Martin"}}"#.into(),
            ),
            (
                r##"{"user":{"color":"#B4462B"}}"##.into(),
                r##"{"user":{"color":"#B4462B","name":"Mallory Martin"}}"##.into(),
            ),
            (r#"{"user":{}}"#.into(), r#"{"user":{"name":"Mallory Martin"}}"#.into()),
            // Un nom que la règle refuse ne fait plus ignorer l'état : il est remplacé.
            (
                r##"{"user":{"name":"","color":"#B4462B"}}"##.into(),
                r##"{"user":{"name":"Mallory Martin","color":"#B4462B"}}"##.into(),
            ),
            (
                format!(r#"{{"user":{{"name":"{}"}}}}"#, "a".repeat(101)),
                r#"{"user":{"name":"Mallory Martin"}}"#.into(),
            ),
            (
                r#"{"user":{"name":"Mo\u0000na"}}"#.into(),
                r#"{"user":{"name":"Mallory Martin"}}"#.into(),
            ),
            (
                r#"{"user":{"name":7}}"#.into(),
                r#"{"user":{"name":"Mallory Martin"}}"#.into(),
            ),
            (
                r#"{"user":{"name":null}}"#.into(),
                r#"{"user":{"name":"Mallory Martin"}}"#.into(),
            ),
            // Une clé répétée : la dernière fait foi, et elle seule est réécrite.
            (
                r##"{"user":{"name":"A","color":"red;x"},"user":{"name":"B","color":"#B4462B"}}"##
                    .into(),
                r##"{"user":{"name":"Mallory Martin","color":"#B4462B"}}"##.into(),
            ),
        ];
        for (announced, expected) in cases {
            assert_eq!(
                known_shape(&announced, account).as_deref(),
                Some(expected.as_str()),
                "{announced}"
            );
        }
        // Le curseur reste tel que le client l'a écrit.
        let kept = parsed(&known_shape(&in_text, account).unwrap());
        assert_eq!(kept["cursor"], with_cursor["cursor"]);
        assert_eq!(kept["user"]["name"], "Mallory Martin");
        assert_eq!(kept["user"]["color"], "#B4462B");
        // Sans compte, rien ne change : le nom annoncé est jugé, et gardé.
        assert_eq!(
            known_shape(r#"{"user":{"name":"Alice"}}"#, None).as_deref(),
            Some(r#"{"user":{"name":"Alice"}}"#)
        );
        assert_eq!(known_shape(r#"{"user":{"name":""}}"#, None), None);
    }

    /// Avec un compte, le reste de la forme est jugé comme avant : la couleur, le curseur, et
    /// aucune autre clé, ni dans l'état, ni dans `user`, qui reste un objet. Et la taille du
    /// texte reçu.
    #[test]
    fn with_an_account_the_rest_of_the_shape_is_still_judged() {
        let account = Some("Mallory");
        let too_big = format!(r#"{{"user":{{"name":"{}"}}}}"#, " ".repeat(2048));
        for state in [
            r#"{"user":{"name":"Mona","color":"red;position:fixed"}}"#.to_string(),
            r#"{"user":{"name":"Mona","avatar":"x"}}"#.into(),
            r#"{"user":null}"#.into(),
            r#"{"user":"Mona"}"#.into(),
            r#"{"surprise":true}"#.into(),
            r#"{"cursor":{"anchor":7,"head":7}}"#.into(),
            format!(r#"{{"cursor":{{"anchor":{END}}}}}"#),
            "[]".into(),
            "{".into(),
            too_big,
        ] {
            assert_eq!(known_shape(&state, account), None, "{state}");
        }
        // Le nom du compte peut faire dépasser 2 Kio à l'état réécrit : la borne est celle du
        // texte reçu, et l'état reste lisible par tous.
        let longest = display_name(&"🚀".repeat(100));
        let padded = format!(
            r#"{{"user":{{}}{}}}"#,
            " ".repeat(2048 - r#"{"user":{}}"#.len())
        );
        assert_eq!(padded.len(), 2048);
        let kept = known_shape(&padded, Some(&longest)).expect("un état de 2 Kio");
        assert_eq!(parsed(&kept)["user"]["name"], longest.as_str());
    }

    /// Un client que son compte a tenu lui reste quand sa dernière connexion part : la table le
    /// garde, sans connexion vivante. Le compte le reprend ; d'autres connexions du même compte
    /// le tiennent avec la première.
    #[test]
    fn holders_keep_a_released_client_for_its_account() {
        let (alice, bob) = (Some(Uuid::now_v7()), Some(Uuid::now_v7()));
        let (tab, other) = (ClientID::new(7), ClientID::new(8));
        let mut holders = Holders::default();
        assert_eq!(holders.get(tab), None);
        holders.take(tab, alice);
        holders.take(other, bob);
        let live = |account| {
            Some(Holding {
                account,
                live: true,
            })
        };
        let kept = |account| {
            Some(Holding {
                account,
                live: false,
            })
        };
        assert_eq!(holders.get(tab), live(alice));
        // Deux connexions d'Alice le tiennent : il reste tenu tant que l'une vit.
        holders.take(tab, alice);
        assert!(!holders.release(tab));
        assert_eq!(holders.get(tab), live(alice));
        assert!(
            holders.release(tab),
            "la dernière connexion qui le tenait part"
        );
        assert_eq!(holders.get(tab), kept(alice));
        assert_eq!(holders.remembered(), 1);
        // Relâché de nouveau : rien, il n'était plus tenu.
        assert!(!holders.release(tab));
        assert_eq!(holders.remembered(), 1);
        // Alice le reprend.
        holders.take(tab, alice);
        assert_eq!(holders.get(tab), live(alice));
        assert_eq!(holders.remembered(), 0);
        assert_eq!(holders.get(other), live(bob));
    }

    /// Au-delà de [`MAX_REMEMBERED_CLIENTS`] clients retenus sans connexion vivante, la table
    /// oublie d'abord ceux du compte qui en a le plus, le plus ancien départ d'abord : un compte
    /// qui la remplit n'y fait oublier que les siens. Entre comptes à égalité, le plus ancien
    /// départ part.
    #[test]
    fn holders_forget_first_the_account_with_the_most() {
        assert_eq!(MAX_REMEMBERED_CLIENTS, 4096);
        let (alice, bob, mallory) = (
            Some(Uuid::now_v7()),
            Some(Uuid::now_v7()),
            Some(Uuid::now_v7()),
        );
        let mut holders = Holders::default();
        let leave = |holders: &mut Holders, client: u64, account| {
            let client = ClientID::new(client);
            holders.take(client, account);
            assert!(holders.release(client));
            holders.forget_beyond_bound();
        };
        // Alice et Bob partent les premiers ; puis Mallory, autant qu'elle veut.
        leave(&mut holders, 1, alice);
        leave(&mut holders, 2, bob);
        for client in 0..MAX_REMEMBERED_CLIENTS as u64 + 50 {
            leave(&mut holders, 1_000 + client, mallory);
        }
        assert_eq!(holders.remembered(), MAX_REMEMBERED_CLIENTS);
        // Alice et Bob sont toujours retenus ; Mallory a perdu ses 52 plus anciens.
        for (client, account) in [(1, alice), (2, bob)] {
            assert_eq!(
                holders.get(ClientID::new(client)).map(|h| h.account),
                Some(account)
            );
        }
        for client in 1_000..1_052 {
            assert_eq!(holders.get(ClientID::new(client)), None, "{client}");
        }
        for client in [1_052, 1_000 + MAX_REMEMBERED_CLIENTS as u64 + 49] {
            assert!(holders.get(ClientID::new(client)).is_some(), "{client}");
        }

        // Des comptes à égalité, un client chacun : le plus ancien départ part le premier.
        let mut holders = Holders::default();
        for client in 0..MAX_REMEMBERED_CLIENTS as u64 + 2 {
            leave(&mut holders, client, Some(Uuid::now_v7()));
        }
        assert_eq!(holders.remembered(), MAX_REMEMBERED_CLIENTS);
        assert_eq!(holders.get(ClientID::new(0)), None);
        assert_eq!(holders.get(ClientID::new(1)), None);
        assert!(holders.get(ClientID::new(2)).is_some());
    }

    /// Un client tenu par une connexion vivante n'est jamais oublié, et ne compte pas dans la
    /// borne : des connexions vivantes en tiennent autant qu'elles veulent (huit chacune).
    #[test]
    fn holders_never_forget_a_live_client() {
        let mallory = Some(Uuid::now_v7());
        let mut holders = Holders::default();
        let count = 2 * MAX_REMEMBERED_CLIENTS as u64;
        for client in 0..count {
            holders.take(ClientID::new(client), mallory);
        }
        holders.forget_beyond_bound();
        assert_eq!(holders.remembered(), 0);
        // Tous partent : la borne s'applique à ceux qui restent sans connexion.
        for client in 0..count {
            assert!(holders.release(ClientID::new(client)));
        }
        holders.forget_beyond_bound();
        assert_eq!(holders.remembered(), MAX_REMEMBERED_CLIENTS);
        // Un client repris est de nouveau vivant : jamais oublié, quoi qu'il parte ensuite.
        let last = ClientID::new(count - 1);
        holders.take(last, mallory);
        for client in count..count + 100 {
            holders.take(ClientID::new(client), mallory);
            holders.release(ClientID::new(client));
            holders.forget_beyond_bound();
        }
        assert_eq!(
            holders.get(last),
            Some(Holding {
                account: mallory,
                live: true
            })
        );
    }
}
