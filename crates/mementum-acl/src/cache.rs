//! Les accès déjà calculés, par personne et par espace, valables pour une révision de l'espace.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

use crate::SpaceAccess;

type Entries = HashMap<(Uuid, Uuid), (i64, Arc<SpaceAccess>)>;

/// Les [`SpaceAccess`] déjà calculés, par `(personne, espace)`, chacun avec la révision de
/// l'espace (`spaces.rev`) à laquelle il a été calculé. Une autre révision est un échec : le
/// store incrémente `rev` dans la transaction de toute écriture qui touche à la structure ou
/// aux droits de l'espace.
///
/// L'appelant lit la révision **avant** les données du calcul, ou dans la même transaction.
/// Lue après, elle pourrait couvrir un calcul fait sur des données plus anciennes, et un droit
/// retiré entre-temps resterait en cache.
///
/// Plein, il se vide d'un coup avant de garder une nouvelle entrée. Il se partage entre tâches
/// (`Send + Sync`) : le verrou n'est tenu que le temps d'une lecture ou d'une écriture.
pub struct AccessCache {
    capacity: usize,
    entries: Mutex<Entries>,
}

// Le serveur partage le cache entre tâches tokio.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<AccessCache>();
};

impl AccessCache {
    /// Un cache d'au plus `capacity` entrées (`0` : il ne garde rien).
    pub fn new(capacity: usize) -> AccessCache {
        AccessCache {
            capacity,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Les accès de `user` dans `space`, s'ils ont été calculés à la révision `rev`.
    pub fn get(&self, user: Uuid, space: Uuid, rev: i64) -> Option<Arc<SpaceAccess>> {
        match self.lock().get(&(user, space)) {
            Some((at, access)) if *at == rev => Some(Arc::clone(access)),
            _ => None,
        }
    }

    /// Garde `access`, calculé à la révision `rev`, à la place de ce que le cache avait pour
    /// `(user, space)`.
    pub fn put(&self, user: Uuid, space: Uuid, rev: i64, access: Arc<SpaceAccess>) {
        if self.capacity == 0 {
            return;
        }
        let key = (user, space);
        let mut entries = self.lock();
        if entries.len() >= self.capacity && !entries.contains_key(&key) {
            entries.clear();
        }
        entries.insert(key, (rev, access));
    }

    /// Un verrou empoisonné (panique pendant un autre appel) ne laisse pas la table dans un
    /// état faux : chaque écriture est un seul `insert` ou `clear`.
    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
