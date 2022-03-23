mod proto;

use crate::proto::galadh::kv_client::KvClient;
use crate::proto::galadh::{PutRequest, PutResponse, RangeRequest, RangeResponse};
use tonic::transport;

use env_logger;
use std::env;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Get {
        key: String,
        #[clap(long)]
        prefix: bool,
    },
    Put {
        key: String,
        val: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if let Err(_) = env::var("RUST_LOG") {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    let mut client = KvClient::connect("http://127.0.0.1:2379").await.unwrap();
    match &cli.command {
        Commands::Get { key, prefix } => {
            log::debug!("args: key={key}, prefix={prefix}");
            let req = tonic::Request::new(RangeRequest {
                key: key.clone().into_bytes(),
                range_end: key.clone().into_bytes(),
                limit: 100,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            });
            let resp = client.range(req).await;
            match resp {
                Ok(res) => {
                    for kv in res.into_inner().kvs.into_iter() {
                        println!(
                            "{}\n{}",
                            String::from_utf8(kv.key)?,
                            String::from_utf8(kv.value)?
                        );
                    }
                }
                Err(err) => {
                    log::error!("Err getting key {}: {}", &key, err)
                }
            }
        }
        Commands::Put { key, val } => {
            let req = tonic::Request::new(PutRequest {
                key: key.clone().into_bytes(),
                value: val.clone().into_bytes(),
                lease: 0,
                prev_kv: true,
                ignore_value: true,
                ignore_lease: true,
            });
            let response = client.put(req).await;
        }
    }

    Ok(())
}
