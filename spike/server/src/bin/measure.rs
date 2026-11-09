//! Mesures de la Task 1 : page de 10 000 blocs, mémoire au repos, démarrage à froid, chargement.
//! À lancer depuis `spike/` : `cargo run --release -p spike-server --bin measure`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use spike_server::rooms::FRAGMENT;
use spike_server::store::{new_doc, Store};
use yrs::{ReadTxn, StateVector, Transact, XmlElementPrelim, XmlFragment, XmlTextPrelim, Xml};

const PAGE: &str = "bench-10k";
const BLOCKS: usize = 10_000;

fn build_page() -> Vec<u8> {
    let doc = new_doc();
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    let group = frag.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
    for i in 0..BLOCKS {
        let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", format!("bench-{i}"));
        let para = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        para.insert_attribute(&mut txn, "textAlignment", "left");
        let text = format!("Bloc {i:05} : une ligne de texte de quatre-vingts caractères environ pour mesurer.");
        para.push_back(&mut txn, XmlTextPrelim::new(&text[..text.len().min(80)]));
    }
    drop(txn);
    doc.transact().encode_state_as_update_v1(&StateVector::default())
}

fn health_ok() -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&"127.0.0.1:8080".parse().unwrap(), Duration::from_millis(50)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
    if s.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.ends_with("ok")
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = spike_server::db_path();
    let store = Store::open(&db).await?;

    let state = build_page();
    store.put_snapshot(PAGE, &state).await?;
    println!("page `{PAGE}` : {BLOCKS} blocs, instantané de {} Ko", state.len() / 1024);

    // Chargement depuis SQLite (page « jamais vue » côté serveur).
    let mut times = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let doc = store.load_doc(PAGE).await?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(doc);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("chargement de la page : p50 {:.1} ms, p95 {:.1} ms", percentile(&times, 0.5), percentile(&times, 0.95));

    // Démarrage à froid et mémoire au repos du binaire serveur.
    let exe = std::env::current_exe()?.with_file_name(format!("spike-server{}", std::env::consts::EXE_SUFFIX));
    let t = Instant::now();
    let mut child = Command::new(&exe).env("SPIKE_DB", &db).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    while !health_ok() {
        if t.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            anyhow::bail!("le serveur ne répond pas sur :8080 (port déjà pris ?)");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let cold = t.elapsed();
    std::thread::sleep(Duration::from_secs(2));
    let mut sys = sysinfo::System::new().map_err(|e| anyhow::anyhow!("sysinfo : {e:?}"))?;
    let pid = sysinfo::Pid::from_u32(child.id());
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    let rss_mb = sys.process(pid).map(|p| p.memory() as f64 / 1_048_576.0).unwrap_or(f64::NAN);
    let _ = child.kill();
    println!("démarrage à froid : {:.0} ms", cold.as_secs_f64() * 1000.0);
    println!("mémoire au repos : {rss_mb:.1} Mo");
    Ok(())
}
