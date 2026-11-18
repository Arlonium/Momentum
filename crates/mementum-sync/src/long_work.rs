//! Les calculs de yrs dont la durée dépend de ce qu'un client a écrit (Ruling R863).
//!
//! yrs range un élément en parcourant ce qui suit son origine : une mise à jour de 110 Ko posée sur
//! une chaîne de 552 Ko occupe l'acteur de la page 49 s (Ruling R860). L'acteur est une tâche du
//! moteur tokio : tant qu'elle calcule, son fil ne sert plus rien d'autre, et ce qui attendait sur ce
//! fil attend avec elle. Quatre pages attaquées sur quatre fils retenaient `/health` 9,5 s.
//!
//! [`off_the_engine`] rend le fil au moteur le temps du calcul. L'acteur de la page reste occupé :
//! les autres pages et les routes continuent d'être servies. Borner ce qu'un compte fait calculer à
//! une page reste à faire (le débit par compte, lot 7).

/// Mène `work`, en laissant au moteur tokio de quoi servir ses autres tâches pendant ce temps.
///
/// Sur un moteur à un seul fil (`#[tokio::test]` par défaut), `block_in_place` panique, et il n'y a
/// pas d'autre fil à qui rendre la main : `work` est mené tel quel.
pub(crate) fn off_the_engine<R>(work: impl FnOnce() -> R) -> R {
    use tokio::runtime::{Handle, RuntimeFlavor};
    let work = || {
        #[cfg(test)]
        testing::reached();
        work()
    };
    match Handle::try_current().map(|engine| engine.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

/// Ce qui permet à un test de se tenir à l'intérieur d'un calcul long.
#[cfg(test)]
pub(crate) mod testing {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type Hook = Arc<dyn Fn() + Send + Sync>;

    /// Par nom de fil : les tests d'un même binaire tournent ensemble, chacun sur son moteur.
    static HOOKS: Mutex<Option<HashMap<String, Hook>>> = Mutex::new(None);

    /// `hook` sera appelé au début de chaque calcul long mené par un fil nommé `thread`.
    pub(crate) fn on_long_work(thread: &str, hook: Hook) {
        HOOKS
            .lock()
            .unwrap()
            .get_or_insert_default()
            .insert(thread.to_string(), hook);
    }

    pub(super) fn reached() {
        let thread = std::thread::current();
        let hook = thread.name().and_then(|name| {
            let hooks = HOOKS.lock().unwrap();
            hooks.as_ref()?.get(name).cloned()
        });
        if let Some(hook) = hook {
            hook();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sur le moteur à un seul fil des tests ordinaires, le calcul est mené sans paniquer.
    #[tokio::test]
    async fn work_runs_on_an_engine_of_one_thread() {
        assert_eq!(off_the_engine(|| 7), 7);
    }

    /// Hors de tout moteur aussi.
    #[test]
    fn work_runs_outside_of_any_engine() {
        assert_eq!(off_the_engine(|| 7), 7);
    }
}
