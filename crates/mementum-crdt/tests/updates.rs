//! Mises à jour Yjs échangées avec les clients : diff d'après un vecteur
//! d'état, et entrées illisibles.

use mementum_crdt::{CrdtError, PageDoc};
use mementum_md::{block_text, parse};

#[test]
fn diff_brings_a_peer_up_to_date() {
    let (page, created) = PageDoc::new_page();
    let peer = PageDoc::from_updates([created.as_slice()]).unwrap();
    page.write_blocks(&parse("Un\n\nDeux\n").unwrap()).unwrap();
    let id = page.read_blocks()[0].id.clone();
    page.replace_text(&id, 0, 2, "Premier").unwrap();

    let diff = page.encode_diff(&peer.state_vector()).unwrap();
    peer.apply_update(&diff).unwrap();
    assert_eq!(peer.read_blocks(), page.read_blocks());
    assert_eq!(peer.revision(), page.revision());
    assert_eq!(peer.state_vector(), page.state_vector());
}

#[test]
fn unreadable_input_is_a_decode_error() {
    let (page, _) = PageDoc::new_page();
    let before = page.read_blocks();
    assert!(matches!(
        page.apply_update(&[0xff, 0xff, 0xff]),
        Err(CrdtError::Decode(_))
    ));
    assert!(matches!(
        page.encode_diff(&[0x80; 8]),
        Err(CrdtError::Decode(_))
    ));
    assert!(matches!(
        PageDoc::from_updates([&[0xff, 0xff][..]]),
        Err(CrdtError::Decode(_))
    ));
    assert_eq!(page.read_blocks(), before);
}

/// `varUint` lib0.
fn var(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Mise à jour sans bloc dont le jeu de suppressions vise `client`, de
/// `clock` sur `len` (Ruling R25).
fn deletion(client: u64, clock: u64, len: u64) -> Vec<u8> {
    let mut update = vec![0, 1];
    var(client, &mut update);
    update.push(1);
    var(clock, &mut update);
    var(len, &mut update);
    update
}

#[test]
fn hostile_delete_set_is_a_decode_error() {
    let (page, _) = PageDoc::new_page();
    let before = page.read_blocks();
    // yrs ne les refuse qu'en paniquant en debug (`debug_assert!`, addition
    // qui déborde) : la garde les lit avant lui.
    for (what, update) in [
        ("client hors de 53 bits", deletion(1 << 60, 0, 1)),
        ("client juste au-delà", deletion(1 << 53, 0, 1)),
        ("plage qui déborde", deletion(7, u64::from(u32::MAX), 2)),
    ] {
        assert!(
            matches!(page.apply_update(&update), Err(CrdtError::Decode(_))),
            "{what}"
        );
    }
    assert_eq!(page.read_blocks(), before);
    // Le plus grand client admis passe.
    assert_eq!(page.apply_update(&deletion((1 << 53) - 1, 0, 1)), Ok(()));
}

/// Lit un entier variable comme lib0, dans le navigateur : la valeur entière,
/// sans la couper à 32 bits.
fn read_var(bytes: &[u8], at: &mut usize) -> u128 {
    let (mut value, mut shift) = (0u128, 0u32);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u128::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return value;
        }
        shift += 7;
    }
}

/// Le client, l'horloge et la longueur de la seule plage qu'une mise à jour
/// sans bloc supprime, lus comme un navigateur, jusqu'au dernier octet.
fn read_deletion(update: &[u8]) -> (u128, u128, u128) {
    let mut at = 0;
    assert_eq!(read_var(update, &mut at), 0, "aucun bloc");
    assert_eq!(read_var(update, &mut at), 1, "un client");
    let client = read_var(update, &mut at);
    assert_eq!(read_var(update, &mut at), 1, "une plage");
    let clock = read_var(update, &mut at);
    let len = read_var(update, &mut at);
    assert_eq!(at, update.len(), "des octets après les suppressions");
    (client, clock, len)
}

/// yrs et Yjs ne lisent pas les mêmes octets pareil : yrs laisse tomber les
/// bits d'un entier au-delà de 32, Yjs lit l'entier en entier. Une horloge
/// écrite avec 2^32 de plus désigne donc, pour la page, le texte à supprimer,
/// et pour un navigateur, une plage qu'il ne connaît pas : il la garde en
/// attente, et ne supprime rien. La page rend ce qu'elle a lu, réencodé : c'est
/// cela qui s'écrit et se relaie, jamais les octets reçus (Ruling R130).
#[test]
fn client_update_is_returned_as_the_page_read_it() {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(
        &parse(
            "Bonjour
",
        )
        .unwrap(),
    )
    .unwrap();
    let id = page.read_blocks()[0].id.clone();
    // La suppression honnête de « Bonjour », faite ailleurs : elle dit le
    // client et la plage du texte.
    let peer = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    let honest = peer.replace_text(&id, 0, 7, "").unwrap();
    let (client, clock, len) = read_deletion(&honest);

    let forged = deletion(client as u64, clock as u64 + (1 << 32), len as u64);
    assert_eq!(read_deletion(&forged), (client, clock + (1 << 32), len));
    let read = page.apply_client_update(&forged).expect("lisible par yrs");
    assert_eq!(
        block_text(&page.read_blocks()[0]),
        "",
        "la page a supprimé le texte"
    );
    assert_eq!(
        read_deletion(&read),
        (client, clock, len),
        "un navigateur lit ce que la page a supprimé"
    );
    assert_eq!(read, honest);
}

/// Ce qu'un client honnête envoie ressort avec le même effet : une page qui
/// applique ce que la première a rendu est la même page.
#[test]
fn honest_client_update_is_returned_with_the_same_effect() {
    let (page, created) = PageDoc::new_page();
    let client = PageDoc::from_updates([created.as_slice()]).unwrap();
    let mirror = PageDoc::from_updates([created.as_slice()]).unwrap();
    let written = client
        .write_blocks(
            &parse(
                "# Titre

Un **gras**, un [lien](https://exemple.fr).
",
            )
            .unwrap(),
        )
        .unwrap();
    let id = client.read_blocks()[1].id.clone();
    let edited = client.replace_text(&id, 0, 2, "Deux").unwrap();

    for update in [written, edited] {
        let read = page
            .apply_client_update(&update)
            .expect("mise à jour honnête");
        mirror
            .apply_update(&read)
            .expect("réencodée, elle se relit");
    }
    assert_eq!(page.read_blocks(), client.read_blocks());
    assert_eq!(mirror.read_blocks(), page.read_blocks());
    assert_eq!(mirror.revision(), page.revision());
    assert_eq!(mirror.state_vector(), page.state_vector());
}

/// Trois éditions d'un même client, chacune dans son bloc : aucune ne dépend d'une autre.
struct Edits {
    /// L'état de départ de la page, que le client a reçu.
    state: Vec<u8>,
    a1: Vec<u8>,
    a2: Vec<u8>,
    a3: Vec<u8>,
    /// a1 et a3 fusionnées : un `Skip` tient la place de a2, avant les blocs de a3.
    skipping: Vec<u8>,
}

fn three_edits() -> Edits {
    let (base, _) = PageDoc::new_page();
    base.write_blocks(&parse("Alpha\n\nBêta\n\nGamma\n").unwrap())
        .unwrap();
    let state = base.encode_state();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    let client = PageDoc::from_updates([state.as_slice()]).unwrap();
    let a1 = client.replace_text(&ids[0], 5, 0, " un").unwrap();
    let a2 = client.replace_text(&ids[1], 4, 0, " deux").unwrap();
    let a3 = client.replace_text(&ids[2], 5, 0, " trois").unwrap();
    let skipping = yrs::merge_updates_v1([a1.as_slice(), a3.as_slice()]).unwrap();
    Edits {
        state,
        a1,
        a2,
        a3,
        skipping,
    }
}

/// Ruling R131 : un client fait des éditions qui ne dépendent pas l'une de l'autre. La deuxième,
/// arrivée seule, laisserait un trou dans les horloges de son client : yrs l'intégrerait, et le
/// serveur lirait un texte qu'un navigateur, qui la garde en attente, n'affiche pas. La page le
/// dit avant de rien appliquer.
#[test]
fn update_ahead_of_its_client_leaves_a_gap() {
    let Edits {
        state,
        a1,
        a2,
        skipping,
        ..
    } = three_edits();
    let page = PageDoc::from_updates([state.as_slice()]).unwrap();
    let revision = page.revision();
    assert_eq!(page.leaves_gap(&a2), Ok(true), "a2 avant a1");
    assert_eq!(page.leaves_gap(&a1), Ok(false), "a1, la première");
    assert_eq!(
        page.leaves_gap(&skipping),
        Ok(true),
        "un Skip avant un bloc inconnu"
    );
    assert_eq!(
        page.leaves_gap(&state),
        Ok(false),
        "tous ses blocs sont connus"
    );
    assert!(matches!(
        page.leaves_gap(&[0xff, 0xff, 0xff]),
        Err(CrdtError::Decode(_))
    ));
    assert_eq!(page.revision(), revision, "rien n'est appliqué");

    page.apply_client_update(&a1).unwrap();
    let revision = page.revision();
    assert_eq!(page.leaves_gap(&a2), Ok(false), "a1 arrivée, a2 la suit");
    assert_eq!(page.leaves_gap(&a1), Ok(false), "a1, déjà connue");
    assert_eq!(page.leaves_gap(&skipping), Ok(true), "a3 attend encore a2");
    assert_eq!(page.revision(), revision, "rien n'est appliqué");

    page.apply_client_update(&a2).unwrap();
    assert_eq!(
        page.leaves_gap(&skipping),
        Ok(false),
        "a2 arrivée, a3 la suit"
    );
}

/// Ce qu'attend une mise à jour retenue se compare à la page sans la relire : la page qui l'a
/// atteint la prend sans trou, et ne le perd plus.
#[test]
fn what_an_update_awaits_is_reached_once_its_gap_is_filled() {
    let Edits {
        state,
        a1,
        a2,
        a3,
        skipping,
    } = three_edits();
    let page = PageDoc::from_updates([state.as_slice()]).unwrap();
    let revision = page.revision();
    assert_eq!(page.awaits(&a1), Ok(None), "a1 n'attend rien");
    assert_eq!(page.awaits(&state), Ok(None), "tous ses blocs sont connus");
    assert!(matches!(
        page.awaits(&[0xff, 0xff, 0xff]),
        Err(CrdtError::Decode(_))
    ));
    let after_a1 = page.awaits(&a2).unwrap().expect("a2 attend a1");
    let after_a2 = page.awaits(&skipping).unwrap().expect("a3 attend a2");
    assert_eq!(page.revision(), revision, "rien n'est appliqué");
    assert!(!page.has_reached(&after_a1));
    assert!(!page.has_reached(&after_a2));

    page.apply_client_update(&a1).unwrap();
    assert!(page.has_reached(&after_a1));
    assert!(!page.has_reached(&after_a2), "a1 ne suffit pas à a3");
    assert_eq!(page.leaves_gap(&a2), Ok(false));

    page.apply_client_update(&a2).unwrap();
    assert!(page.has_reached(&after_a2));
    assert_eq!(page.leaves_gap(&skipping), Ok(false));
    // Atteint, il le reste.
    page.apply_client_update(&a3).unwrap();
    assert!(page.has_reached(&after_a1) && page.has_reached(&after_a2));
}

/// Ruling R198 : une suppression qui vise des horloges que la page n'a pas attend comme un trou.
/// yrs la garderait en attente, et supprimerait à leur arrivée les éléments que son client y
/// écrirait. Les blocs de la même mise à jour comptent : une mise à jour qui écrit puis supprime
/// ses propres éléments n'attend rien. Une fois la page arrivée à ces horloges, la suppression
/// s'applique sans rien laisser en attente.
#[test]
fn deletion_ahead_of_the_page_awaits_its_clocks() {
    let Edits { state, a1, .. } = three_edits();
    let page = PageDoc::from_updates([state.as_slice()]).unwrap();
    let id = page.read_blocks()[0].id.clone();
    let client = PageDoc::from_updates([state.as_slice(), a1.as_slice()]).unwrap();
    let erase = client.replace_text(&id, 5, 3, "").unwrap();
    let both = yrs::merge_updates_v1([a1.as_slice(), erase.as_slice()]).unwrap();

    assert_eq!(
        page.leaves_gap(&erase),
        Ok(true),
        "l'effacement avant le texte"
    );
    let awaited = page.awaits(&erase).unwrap().expect("il attend le texte");
    assert_eq!(page.awaits(&both), Ok(None), "le texte et son effacement");
    assert_eq!(page.leaves_gap(&both), Ok(false));

    page.apply_client_update(&a1).unwrap();
    assert!(page.has_reached(&awaited));
    assert_eq!(page.awaits(&erase), Ok(None));
    page.apply_client_update(&erase).unwrap();
    assert!(!page.has_pending_deletions());
    assert_eq!(block_text(&page.read_blocks()[0]), "Alpha");
}

/// Ruling R199 : une page relue d'une base d'avant R198 peut garder une suppression en attente.
/// La renvoyer n'attend rien (chaque client la renvoie dans son étape 2) ; ce qui la dépasse
/// attend, comme ailleurs.
#[test]
fn deletion_already_pending_awaits_nothing() {
    let Edits { state, .. } = three_edits();
    let pending = deletion(4242, 10, 5);
    let page = PageDoc::from_updates([state.as_slice(), pending.as_slice()]).unwrap();
    assert!(page.has_pending_deletions());
    assert_eq!(page.awaits(&pending), Ok(None), "le renvoi");
    assert_eq!(page.awaits(&deletion(4242, 11, 2)), Ok(None), "une partie");
    assert_eq!(page.leaves_gap(&pending), Ok(false));
    assert!(
        page.awaits(&deletion(4242, 10, 6)).unwrap().is_some(),
        "au-delà"
    );
    assert!(
        page.awaits(&deletion(4242, 8, 3)).unwrap().is_some(),
        "en deçà"
    );
    assert!(
        page.awaits(&deletion(4243, 10, 5)).unwrap().is_some(),
        "un autre client"
    );
}
