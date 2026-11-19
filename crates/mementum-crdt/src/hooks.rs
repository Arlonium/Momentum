//! Ce qu'un test voit de ce que la page donne à yrs (fonction `test-hooks`, Ruling R875).
//!
//! Le serveur compile un yrs corrigé (`vendor/yrs`). Pour comparer ce yrs à celui de crates.io
//! sur ce qu'une page lui donne vraiment, un test relève ici chaque mise à jour qu'une page a
//! intégrée et gardée : **les octets tels que yrs les a reçus** (après la coupe et les règles qui
//! récrivent), et l'état de la page juste avant. Rien de cela n'est compilé sans la
//! fonction, que seuls les tests de la crate activent.

use std::cell::RefCell;

use yrs::{ReadTxn, StateVector};

/// Reçoit l'état de la page avant une mise à jour, et la mise à jour telle que yrs la reçoit.
pub type Watcher = Box<dyn FnMut(&[u8], &[u8])>;

thread_local! {
    static WATCHER: RefCell<Option<Watcher>> = const { RefCell::new(None) };
    static HANDED: RefCell<Option<(Vec<u8>, Vec<u8>)>> = const { RefCell::new(None) };
}

/// Pose (ou retire, avec `None`) le témoin du fil courant. Il est appelé pour toute page de ce
/// fil, une fois la mise à jour gardée.
pub fn watch_what_yrs_is_handed(watcher: Option<Watcher>) {
    WATCHER.with(|slot| *slot.borrow_mut() = watcher);
}

/// Une mise à jour lue et jugée bonne à intégrer : retenue jusqu'à ce qu'on sache si la page l'a
/// gardée ([`settle`]).
pub(crate) fn handed<T: ReadTxn>(txn: &T, update: &[u8]) {
    if WATCHER.with(|slot| slot.borrow().is_none()) {
        return;
    }
    let state = txn.encode_state_as_update_v1(&StateVector::default());
    HANDED.with(|slot| *slot.borrow_mut() = Some((state, update.to_vec())));
}

/// La fin d'une intégration : la mise à jour retenue va au témoin si la page l'a gardée. Une mise
/// à jour refusée, même après avoir été appliquée, ne lui est pas donnée : la page est alors à
/// jeter, et rien ne dit qu'un yrs la relit.
pub(crate) fn settle(kept: bool) {
    let handed = HANDED.with(|slot| slot.borrow_mut().take());
    if let (true, Some((state, update))) = (kept, handed) {
        WATCHER.with(|slot| {
            if let Some(watcher) = slot.borrow_mut().as_mut() {
                watcher(&state, &update);
            }
        });
    }
}
