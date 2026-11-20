//! `mementum reindex` (lot 4, Task S7 ; conception §5.7 ; plan du lot, « Reconstruction,
//! sauvegardes ») : l'index de recherche reconstruit, serveur arrêté.
//!
//! **Serveur arrêté seulement** (Ruling R913). Un serveur qui tourne tient le verrou de
//! l'écrivain de l'index plein texte dès son démarrage : la commande prend ce verrou **avant
//! d'ouvrir la base**, si bien que son refus (`IndexError::Busy`) ne laisse rien de modifié. Deux
//! processus sur une base feraient deux indexeurs que rien ne sépare : le verrou de l'indexeur
//! (Ruling R975) et la file d'écriture sont propres à un `Store`.
//!
//! **Dans l'ordre :**
//! 1. l'index plein texte vidé (`SearchIndex::reset`), le verrou de son écrivain tenu jusqu'à la
//!    fin ;
//! 2. la base ouverte par `Store::open`, donc par la règle des copies d'avant migration
//!    (`backup.rs`) : une base à migrer est copiée d'abord, une base refusée reste telle quelle ;
//! 3. l'intention posée défaite : l'index qu'elle visait vient d'être vidé (Ruling R978) ;
//! 4. sauf `--text-only`, chaque page relue depuis son document : toutes redeviennent jamais
//!    indexées, un échec de lecture oublié (`Store::reproject_everything`), puis chacune est
//!    relue, projetée et écrite par ce que fait une indexation du serveur
//!    (`mementum_server::index_stored`), et la version de la projection est écrite (R960) ;
//! 5. toutes les pages nommées dans l'outbox (`Store::outbox_everything`), **avant toute
//!    validation** (Ruling R1001) : un arrêt en route laisse un index sans jeton, ou validé sous
//!    un jeton que la base connaît, quand toutes les pages sont déjà nommées, et le serveur
//!    reprend de là ;
//! 6. les passages, jusqu'à ce que l'outbox soit vide : l'intention, les entrées écrites d'après
//!    la base (`mementum_server::write_lines`), la validation sous le jeton de l'intention, la
//!    clôture (Ruling R909). À la fin, les jetons de la base et de l'index concordent : un
//!    serveur lancé ensuite ne reconstruit rien.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use mementum_search::{IndexError, PROJECTION_VERSION, SearchIndex};
use mementum_server::{Backup, Reindexed, SEARCH_DIR, config};
use mementum_store::{FeedStart, Store, StoreError};
use uuid::Uuid;

/// Pages par tranche : remises à reprojeter, relues, nommées dans l'outbox.
const BATCH: usize = 500;

/// Lignes de l'outbox prises par intention, donc par validation du moteur. Dix fois ce que prend
/// le serveur (`FEED_LINES`, Ruling R1003), qui borne ses validations pour la frappe des autres :
/// serveur arrêté, personne n'attend, et une validation (un segment écrit et vidé sur le disque)
/// coûte de 54 à 204 ms (mesure de S4c). Une ligne pèse moins de deux cents octets en mémoire.
const FEED_LINES: usize = 10_000;

/// Ce que la commande a fait.
pub(crate) struct Report {
    /// Les pages relues depuis leur document : projetées, et illisibles (sorties de l'index,
    /// dites au journal). `None` sous `--text-only`, qui n'en relit aucune.
    pub(crate) pages: Option<(u64, u64)>,
    /// Les entrées écrites dans l'index plein texte.
    pub(crate) entries: u64,
    /// Les validations de l'index plein texte, une par intention.
    pub(crate) commits: u64,
}

/// Reconstruit l'index de recherche des données de `data_dir` (module, « Dans l'ordre »).
/// `origin` est l'origine publique du serveur, relue comme lui la relit (Ruling R998) ; elle ne
/// sert qu'à la projection. `on_backup` reçoit la copie d'avant migration, si la commande a migré
/// la base, que la suite réussisse ou non.
pub(crate) async fn reindex(
    data_dir: &Path,
    text_only: bool,
    origin: Option<Arc<str>>,
    on_backup: impl FnOnce(&Backup),
) -> anyhow::Result<Report> {
    let db = data_dir.join(config::DB_FILE);
    if !db.is_file() {
        anyhow::bail!(
            "aucune base dans le dossier des données {} (attendu : {})",
            data_dir.display(),
            db.display()
        );
    }
    let index = take_index(&data_dir.join(SEARCH_DIR)).await?;
    let store = match Store::open(&db).await {
        Ok(store) => store,
        Err(error) => {
            let opened = anyhow::Error::new(error)
                .context(format!("impossible d'ouvrir la base {}", db.display()));
            close(index).await;
            return Err(opened);
        }
    };
    if let Some(backup) = store.backup() {
        on_backup(backup);
    }
    let done = rebuild(&store, &index, text_only, origin).await;
    store.close().await;
    close(index).await;
    done.context(
        "réindexation interrompue ; relance la commande, serveur arrêté (ce qui est fait reste \
         fait, et un serveur lancé entre-temps reprend l'index de lui-même)",
    )
}

/// L'index plein texte de `dir`, vidé, avec le verrou de son écrivain : pris avant d'ouvrir la
/// base, pour ne rien modifier quand un serveur tourne (Ruling R913). Un dossier qui porte autre
/// chose qu'un index n'est pas touché (Ruling R990).
async fn take_index(dir: &Path) -> anyhow::Result<Arc<SearchIndex>> {
    let at = dir.to_path_buf();
    let reset = tokio::task::spawn_blocking(move || SearchIndex::reset(&at))
        .await
        .context("ouverture de l'index de recherche")?;
    match reset {
        Ok(opened) => Ok(Arc::new(opened.index)),
        Err(IndexError::Busy) => anyhow::bail!(
            "{} ; arrête-le d'abord, puis relance la commande : la réindexation ne tourne que \
             serveur arrêté. Rien n'a été modifié, ni la base ni l'index",
            IndexError::Busy
        ),
        Err(IndexError::NotAnIndex) => anyhow::bail!(
            "impossible de vider l'index de recherche {} : {} ; supprime ce dossier, après avoir \
             vérifié qu'il ne porte rien d'autre à garder, puis relance la commande",
            dir.display(),
            IndexError::NotAnIndex
        ),
        Err(error) => Err(anyhow::Error::new(error).context(format!(
            "impossible de vider l'index de recherche {}",
            dir.display()
        ))),
    }
}

/// Ferme l'index en attendant ses fusions : après la commande, plus aucun fichier de son dossier
/// n'est à moitié écrit. Ce qui est validé l'est déjà : une fermeture refusée n'est que dite.
async fn close(index: Arc<SearchIndex>) {
    let closed = tokio::task::spawn_blocking(move || match Arc::try_unwrap(index) {
        Ok(index) => index.close(),
        // Encore tenu : lâché par qui le tient, sans attendre les fusions.
        Err(_) => Ok(()),
    })
    .await;
    match closed {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "index de recherche fermé sans attendre ses fusions");
        }
        Err(error) => {
            tracing::warn!(%error, "index de recherche fermé sans attendre ses fusions");
        }
    }
}

/// La base ouverte et l'index vidé : la projection (sauf `text_only`), puis l'index plein texte.
async fn rebuild(
    store: &Store,
    index: &Arc<SearchIndex>,
    text_only: bool,
    origin: Option<Arc<str>>,
) -> anyhow::Result<Report> {
    store
        .undo_feed()
        .await
        .context("l'intention posée de l'index n'a pas pu être défaite")?;
    let pages = if text_only {
        None
    } else {
        Some(reproject(store, origin).await?)
    };
    tracing::info!("réindexation : toutes les pages passent à l'index plein texte");
    let mut after = None;
    while let Some(next) = store
        .outbox_everything(after, BATCH)
        .await
        .context("les pages n'ont pas pu être nommées pour l'index plein texte")?
    {
        after = Some(next);
    }
    let (entries, commits) = feed(store, index).await?;
    Ok(Report {
        pages,
        entries,
        commits,
    })
}

/// Chaque page relue depuis son document, projetée et écrite dans `blocks_index` et
/// `page_links` ; rend les pages projetées et les pages illisibles.
async fn reproject(store: &Store, origin: Option<Arc<str>>) -> anyhow::Result<(u64, u64)> {
    tracing::info!("réindexation : chaque page est relue depuis son document");
    let mut after = None;
    while let Some(next) = store
        .reproject_everything(after, BATCH)
        .await
        .context("les pages n'ont pas pu être remises à reprojeter")?
    {
        after = Some(next);
    }
    // Toutes sont jamais indexées, donc en retard : le rattrapage les rend toutes, par tranches
    // dans l'ordre des identifiants, chaque tranche lue une fois.
    let (mut projected, mut unreadable) = (0, 0);
    let mut after = None;
    loop {
        let stale = store
            .stale_pages(after, BATCH)
            .await
            .context("lecture des pages à relire")?;
        for page in stale.pages {
            match index_page(store, origin.clone(), page).await? {
                Some(Reindexed::Projected) => projected += 1,
                Some(Reindexed::Unreadable) => unreadable += 1,
                None => {}
            }
        }
        tracing::info!(
            pages = projected + unreadable,
            "réindexation : pages relues"
        );
        match stale.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    store
        .set_projection_version(PROJECTION_VERSION)
        .await
        .context("écriture de la version de la projection")?;
    Ok((projected, unreadable))
}

/// Indexe une page dans une tâche à elle, comme l'indexeur du serveur : une relecture qui panique
/// (yrs devant un journal hostile) la fait sortir de l'index comme une page illisible (Ruling
/// R995), au lieu d'arrêter la commande. `None` : la page n'existe plus.
async fn index_page(
    store: &Store,
    origin: Option<Arc<str>>,
    page: Uuid,
) -> anyhow::Result<Option<Reindexed>> {
    let task = {
        let store = store.clone();
        tokio::spawn(async move { mementum_server::index_stored(&store, origin, page).await })
    };
    let done = match task.await {
        Ok(done) => done,
        Err(_) => mementum_server::index_unreadable(store, page, "son indexation a paniqué")
            .await
            .map(|_| Reindexed::Unreadable),
    };
    match done {
        Ok(done) => Ok(Some(done)),
        Err(StoreError::NotFound) => Ok(None),
        Err(error) => Err(anyhow::Error::new(error)
            .context(format!("indexation de la page {page} refusée par la base"))),
    }
}

/// Les passages de l'outbox à l'index plein texte, jusqu'à ce qu'elle soit vide (Ruling R909) ;
/// rend les entrées écrites et les validations. Une erreur arrête tout : l'intention reste posée,
/// et le serveur la rejoue ou la défait à son démarrage, selon le jeton que l'index a validé.
async fn feed(store: &Store, index: &Arc<SearchIndex>) -> anyhow::Result<(u64, u64)> {
    let (mut entries, mut commits) = (0, 0);
    loop {
        let feed = match store
            .begin_feed(FEED_LINES)
            .await
            .context("intention de l'index refusée par la base")?
        {
            FeedStart::Nothing => return Ok((entries, commits)),
            FeedStart::Begun(feed) => feed,
            // Défaite au début, et le verrou de l'index tenu ici : un autre processus écrit dans
            // la base à côté de la commande.
            FeedStart::Pending(_) => anyhow::bail!(
                "une intention de l'index de recherche a été posée par un autre processus \
                 pendant la réindexation"
            ),
        };
        entries += mementum_server::write_lines(store, index, &feed.lines)
            .await
            .context("écriture des entrées de l'index de recherche")?;
        let (writer, token) = (index.clone(), feed.token.clone());
        tokio::task::spawn_blocking(move || writer.commit(&token))
            .await
            .context("la validation de l'index de recherche a paniqué")?
            .context("validation de l'index de recherche refusée")?;
        store
            .finish_feed(&feed.token)
            .await
            .context("clôture de l'intention de l'index refusée par la base")?;
        commits += 1;
    }
}
