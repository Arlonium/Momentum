#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    let db = spike_server::db_path();
    eprintln!("spike-server : http://localhost:8080 (base {})", db.display());
    spike_server::serve(listener, db).await
}
