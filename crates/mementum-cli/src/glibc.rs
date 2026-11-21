//! Le seuil de `mmap` de la glibc, fixé au démarrage du binaire sous Linux (défaut D1 du lot 5,
//! Ruling R1556).
//!
//! Après 500 lectures d'agent, la mémoire que le serveur tenait en propre au repos valait 191,7 Mo
//! sur la CI (`malloc_trim` fait après l'arrêt des acteurs, R1526), pour un budget de 150 Mo
//! (§5.10). Dans le même passage (R1547), le même serveur lancé sous `MALLOC_MMAP_THRESHOLD_=131072`
//! en tenait 108,4, sans coût mesuré sur les lectures (17,07 s contre 17,11 s), avec une pointe plus
//! basse (548,6 Mo 2 s après les lectures, contre 664,7) ; moins d'arènes (`MALLOC_ARENA_MAX` à 1
//! ou 2) en laissait 144 à 149. Un seuil fixe éteint le seuil dynamique de la glibc, qui monte à
//! chaque grand bloc rendu par `munmap` (jusqu'à 32 Mio) : les blocs de plus de 128 Kio (documents,
//! encodages, empreintes des pages lues) vont et viennent alors par `mmap` et `munmap` au lieu de
//! rester, morcelés, dans les arènes que `malloc_trim` ne rend qu'en partie (l'explication est
//! probable ; la mesure est le fait).
//!
//! Le réglage de l'exploitant l'emporte : `MALLOC_MMAP_THRESHOLD_`, ou
//! `glibc.malloc.mmap_threshold` dans `GLIBC_TUNABLES`, que la glibc a lus à son lancement ; le
//! binaire n'y touche pas.

use std::ffi::OsStr;

/// Le seuil fixé, en octets : 128 Kio, la valeur que les sondes ont mesurée (R1547).
const MMAP_THRESHOLD: libc::c_int = 128 * 1024;

/// Ce que [`set_mmap_threshold`] a fait, dit au journal par [`log`] une fois celui-ci ouvert.
pub(crate) enum MmapThreshold {
    /// Fixé par le binaire.
    Set,
    /// Laissé au réglage de l'exploitant, posé dans l'environnement.
    LeftToOperator,
    /// Refusé par la glibc (elle ne refuse qu'un seuil de plus de la moitié de la taille d'un tas,
    /// 32 Mio sur 64 bits).
    Refused,
}

/// Fixe le seuil, sauf si l'environnement le règle. À appeler en tête de `main`, avant tout fil et
/// toute grande allocation : le seuil vaut pour les allocations qui le suivent.
pub(crate) fn set_mmap_threshold() -> MmapThreshold {
    let variable = std::env::var_os("MALLOC_MMAP_THRESHOLD_");
    let tunables = std::env::var_os("GLIBC_TUNABLES");
    if set_by_operator(variable.as_deref(), tunables.as_deref()) {
        return MmapThreshold::LeftToOperator;
    }
    // SAFETY : `mallopt` n'a aucune précondition ; il change un réglage de la glibc sous le verrou
    // de son arène principale, et ne touche à aucune allocation vivante. Il est appelé avant que le
    // processus ait un second fil.
    match unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, MMAP_THRESHOLD) } {
        1 => MmapThreshold::Set,
        _ => MmapThreshold::Refused,
    }
}

/// L'exploitant règle-t-il le seuil : `MALLOC_MMAP_THRESHOLD_` posée (même vide), ou
/// `glibc.malloc.mmap_threshold` parmi les réglages de `GLIBC_TUNABLES` (`nom=valeur`, séparés par
/// des deux-points) ?
fn set_by_operator(variable: Option<&OsStr>, tunables: Option<&OsStr>) -> bool {
    variable.is_some()
        || tunables.is_some_and(|tunables| {
            tunables
                .to_string_lossy()
                .split(':')
                .any(|tunable| tunable.starts_with("glibc.malloc.mmap_threshold="))
        })
}

/// Dit au journal ce que [`set_mmap_threshold`] a fait : au niveau `debug` (cible
/// `mementum::glibc`), sauf un refus, qui ne devrait jamais arriver.
pub(crate) fn log(done: MmapThreshold) {
    match done {
        MmapThreshold::Set => tracing::debug!(
            bytes = MMAP_THRESHOLD,
            "seuil de mmap de la glibc fixé par le binaire (Ruling R1556)"
        ),
        MmapThreshold::LeftToOperator => tracing::debug!(
            "seuil de mmap de la glibc laissé à l'environnement (MALLOC_MMAP_THRESHOLD_ ou \
             GLIBC_TUNABLES)"
        ),
        MmapThreshold::Refused => tracing::warn!(
            bytes = MMAP_THRESHOLD,
            "la glibc a refusé le seuil de mmap : la mémoire que le serveur garde après usage peut \
             rester plus haute"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::set_by_operator;

    /// Le réglage de l'exploitant l'emporte, par la variable ou par les réglages de la glibc ;
    /// un autre réglage de la glibc ne compte pas.
    #[test]
    fn the_setting_of_the_operator_wins() {
        let os = |text: &'static str| Some(OsStr::new(text));
        assert!(!set_by_operator(None, None));
        assert!(set_by_operator(os("262144"), None));
        assert!(set_by_operator(os(""), None));
        assert!(set_by_operator(
            None,
            os("glibc.malloc.mmap_threshold=65536")
        ));
        assert!(set_by_operator(
            None,
            os("glibc.malloc.arena_max=2:glibc.malloc.mmap_threshold=65536")
        ));
        assert!(!set_by_operator(None, os("glibc.malloc.arena_max=2")));
        assert!(!set_by_operator(
            None,
            os("glibc.malloc.mmap_threshold_max=1")
        ));
        assert!(!set_by_operator(None, os("")));
    }
}
