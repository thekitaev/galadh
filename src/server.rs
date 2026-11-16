mod proto;
mod stash;
mod trie;

use crate::proto::galadh::kv_server::KvServer;
use crate::proto::galadh::{lease_server, watch_server};
use crate::stash::Stash;
use clap::Parser;
use std::path::PathBuf;
use tonic::transport::Server;

#[derive(Parser)]
#[clap(name = "gldh", about = "galadh key-value storage server")]
struct Args {
    /// address to listen on
    #[clap(long, default_value = "127.0.0.1:2379")]
    addr: String,
    /// directory for the snapshot file; when set, data survives restarts
    #[clap(long)]
    data_dir: Option<PathBuf>,
}

const SNAPSHOT_FILE: &str = "galadh.snapshot";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    log::info!("starting");

    let stash = match &args.data_dir {
        Some(dir) => {
            let path = dir.join(SNAPSHOT_FILE);
            log::info!("persistence enabled, snapshot at {:?}", path);
            Stash::with_snapshot(path)?
        }
        None => Stash::new(),
    };

    let addr = args.addr.parse()?;

    Server::builder()
        .add_service(KvServer::new(stash.clone()))
        .add_service(lease_server::LeaseServer::new(stash.clone()))
        .add_service(watch_server::WatchServer::new(stash))
        .serve(addr)
        .await?;

    Ok(())
}
