mod proto;
mod stash;

use crate::proto::galadh::kv_server::KvServer;
use crate::stash::Stash;
use env_logger;
use log;
use std::env;
use tonic::{transport::Server, Request, Response, Status};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(_) = env::var("RUST_LOG") {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    log::info!("starting");

    let stash = Stash::new();

    let addr = "127.0.0.1:2379".parse().unwrap();

    Server::builder()
        .add_service(KvServer::new(stash))
        .serve(addr)
        .await?;

    Ok(())
}
