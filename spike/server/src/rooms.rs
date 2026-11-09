//! Une « salle » par page : le document en mémoire, sa présence, ses clients connectés.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{broadcast, Mutex};
use yrs::sync::{Awareness, Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Transact, TransactionMut, XmlFragmentRef};

use crate::store::{Store, COMPACT_EVERY};

pub const FRAGMENT: &str = "document-store";
/// Délai avant de libérer une salle sans client.
pub const IDLE_RELEASE: Duration = Duration::from_secs(60);

/// Message diffusé aux clients d'une salle : (id de la connexion émettrice, octets y-websocket).
pub type Broadcast = (u64, Vec<u8>);

pub struct Room {
    pub page: String,
    pub awareness: Mutex<Awareness>,
    pub tx: broadcast::Sender<Broadcast>,
    /// Révisions lues par les agents (Task 5).
    pub revisions: crate::revisions::RevisionLog,
    store: Store,
    clients: AtomicUsize,
}

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub fn next_conn_id() -> u64 {
    NEXT_CONN.fetch_add(1, Ordering::Relaxed)
}

impl Room {
    /// Persiste une mise à jour déjà appliquée au document, compacte si besoin, puis la diffuse.
    pub async fn commit_update(&self, from_conn: u64, update: Vec<u8>, origin: &str) -> Result<()> {
        let seq = self.store.append_update(&self.page, &update, origin).await?;
        if self.store.pending_updates(&self.page).await? >= COMPACT_EVERY {
            let state = {
                let aw = self.awareness.lock().await;
                let txn = aw.doc().transact();
                txn.encode_state_as_update_v1(&StateVector::default())
            };
            self.store.compact(&self.page, &state, seq).await?;
        }
        let msg = Message::Sync(SyncMessage::Update(update)).encode_v1();
        let _ = self.tx.send((from_conn, msg));
        Ok(())
    }

    /// Modifie le document côté serveur (import, agent…) dans une transaction d'origine `origin`.
    pub async fn transact<R>(&self, origin: &str, f: impl FnOnce(&mut TransactionMut, &XmlFragmentRef) -> R) -> Result<R> {
        let (result, update) = {
            let aw = self.awareness.lock().await;
            let doc = aw.doc();
            let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
            let mut txn = doc.transact_mut_with(origin);
            let r = f(&mut txn, &frag);
            let update = txn.encode_update_v1();
            (r, update)
        };
        if !update.is_empty() {
            self.commit_update(0, update, origin).await?;
        }
        Ok(result)
    }

    /// Lecture cohérente du document (sous le même verrou que les écritures).
    pub async fn read<R>(&self, f: impl FnOnce(&yrs::Transaction<'_>, &XmlFragmentRef) -> R) -> R {
        let aw = self.awareness.lock().await;
        let doc = aw.doc();
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let txn = doc.transact();
        f(&txn, &frag)
    }

    pub fn client_joined(&self) {
        self.clients.fetch_add(1, Ordering::SeqCst);
    }

    /// Renvoie le nombre de clients restants.
    pub fn client_left(&self) -> usize {
        self.clients.fetch_sub(1, Ordering::SeqCst) - 1
    }

    pub fn client_count(&self) -> usize {
        self.clients.load(Ordering::SeqCst)
    }
}

/// Écrit la racine BlockNote (`blockGroup > blockContainer > paragraph`) dans une page vide et
/// renvoie l'état complet à persister. C'est le serveur qui crée les pages : deux clients ne doivent
/// jamais créer chacun leur racine (ProseMirror en supprimerait une, avec son texte).
fn seed_if_empty(doc: &yrs::Doc) -> Option<Vec<u8>> {
    use yrs::{Xml, XmlElementPrelim, XmlFragment};
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut_with("seed");
    if frag.len(&txn) > 0 {
        return None;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let group = frag.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
    let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
    container.insert_attribute(&mut txn, "id", format!("seed-{nanos:x}"));
    let para = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
    para.insert_attribute(&mut txn, "backgroundColor", "default");
    para.insert_attribute(&mut txn, "textAlignment", "left");
    para.insert_attribute(&mut txn, "textColor", "default");
    drop(txn);
    Some(doc.transact().encode_state_as_update_v1(&StateVector::default()))
}

#[derive(Clone)]
pub struct Rooms {
    store: Store,
    rooms: Arc<Mutex<HashMap<String, Arc<Room>>>>,
}

impl Rooms {
    pub fn new(store: Store) -> Rooms {
        Rooms { store, rooms: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub async fn get_or_load(&self, page: &str) -> Result<Arc<Room>> {
        let mut rooms = self.rooms.lock().await;
        if let Some(room) = rooms.get(page) {
            return Ok(room.clone());
        }
        let doc = self.store.load_doc(page).await?;
        if let Some(state) = seed_if_empty(&doc) {
            self.store.put_snapshot(page, &state).await?;
        }
        let (tx, _) = broadcast::channel(1024);
        let room = Arc::new(Room {
            page: page.to_string(),
            awareness: Mutex::new(Awareness::new(doc)),
            tx,
            revisions: Default::default(),
            store: self.store.clone(),
            clients: AtomicUsize::new(0),
        });
        rooms.insert(page.to_string(), room.clone());
        Ok(room)
    }

    /// Libère la salle après `IDLE_RELEASE` si plus aucun client ne l'utilise.
    pub fn schedule_release(&self, room: Arc<Room>) {
        let rooms = self.rooms.clone();
        tokio::spawn(async move {
            tokio::time::sleep(IDLE_RELEASE).await;
            if room.client_count() == 0 {
                rooms.lock().await.remove(&room.page);
            }
        });
    }
}
