//! Les accès déjà calculés, par personne et par espace, valables pour une révision de l'espace.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

use crate::SpaceAccess;

/// Le contenu du cache, sous un seul verrou.
#[derive(Default)]
struct Entries {
    /// `(personne, espace)` → `(révision, accès)`.
    map: HashMap<(Uuid, Uuid), (i64, Arc<SpaceAccess>)>,
    /// La somme des poids ([`weight`]) des entrées gardées.
    nodes: usize,
}

/// Les [`SpaceAccess`] déjà calculés, par `(personne, espace)`, chacun avec la révision de
/// l'espace (`spaces.rev`) à laquelle il a été calculé. Une autre révision est un échec : le
/// store incrémente `rev` dans la transaction de toute écriture qui touche à la structure ou
/// aux droits de l'espace.
///
/// L'appelant lit la révision **avant** les données du calcul, ou dans la même transaction.
/// Lue après, elle pourrait couvrir un calcul fait sur des données plus anciennes, et un droit
/// retiré entre-temps resterait en cache.
///
/// Le cache est borné en nœuds visibles gardés, pas en entrées (Ruling R59) : c'est ce qui
/// pèse en mémoire (une trentaine d'octets par nœud). Un calcul compte pour ses nœuds
/// visibles, et pour un au moins : des calculs vides ne le font pas grossir sans fin. Plein, il
/// se vide d'un coup avant de garder un nouveau calcul ; un calcul plus gros que le plafond, à
/// lui seul, n'est pas gardé. Il se partage entre tâches (`Send + Sync`) : le verrou n'est tenu
/// que le temps d'une lecture ou d'une écriture.
pub struct AccessCache {
    max_nodes: usize,
    entries: Mutex<Entries>,
}

// Le serveur partage le cache entre tâches tokio.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<AccessCache>();
};

/// Ce que compte un calcul dans le plafond du cache : ses nœuds visibles, un au moins.
fn weight(access: &SpaceAccess) -> usize {
    access.visible_count().max(1)
}

impl AccessCache {
    /// Un cache qui garde au plus `max_nodes` nœuds visibles, tous calculs confondus (`0` : il
    /// ne garde rien).
    pub fn new(max_nodes: usize) -> AccessCache {
        AccessCache {
            max_nodes,
            entries: Mutex::new(Entries::default()),
        }
    }

    /// Les accès de `user` dans `space`, s'ils ont été calculés à la révision `rev`.
    pub fn get(&self, user: Uuid, space: Uuid, rev: i64) -> Option<Arc<SpaceAccess>> {
        match self.lock().map.get(&(user, space)) {
            Some((at, access)) if *at == rev => Some(Arc::clone(access)),
            _ => None,
        }
    }

    /// Garde `access`, calculé à la révision `rev`, à la place de ce que le cache avait pour
    /// `(user, space)`. S'il ne tient pas dans ce qui reste, le cache est d'abord vidé ; s'il
    /// est à lui seul plus gros que le plafond, il n'est pas gardé (l'ancienne entrée part
    /// quand même : elle est d'une autre révision).
    pub fn put(&self, user: Uuid, space: Uuid, rev: i64, access: Arc<SpaceAccess>) {
        let key = (user, space);
        let added = weight(&access);
        let mut entries = self.lock();
        if let Some((_, old)) = entries.map.remove(&key) {
            entries.nodes -= weight(&old);
        }
        if added > self.max_nodes {
            return;
        }
        if entries.nodes + added > self.max_nodes {
            entries.map.clear();
            entries.nodes = 0;
        }
        entries.map.insert(key, (rev, access));
        entries.nodes += added;
    }

    /// Un verrou empoisonné (panique pendant un autre appel) ne laisse pas la table dans un
    /// état faux : rien ne panique entre deux écritures qui doivent aller ensemble.
    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
