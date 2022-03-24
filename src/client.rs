mod proto;

use crate::proto::galadh::kv_client::KvClient;
use crate::proto::galadh::{DeleteRangeRequest, PutRequest, RangeRequest};

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
        #[clap(long, default_value_t = 100)]
        limit: usize,
        #[clap(long)]
        keys_only: bool,
        #[clap(long)]
        count_only: bool,
    },
    Put {
        key: String,
        val: String,
    },
    Delete {
        key: String,
        #[clap(long)]
        prefix: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    let mut client = KvClient::connect("http://127.0.0.1:2379").await.unwrap();
    match &cli.command {
        Commands::Get {
            key,
            prefix,
            limit,
            keys_only,
            count_only,
        } => {
            log::debug!("args: key={key}, prefix={prefix}");
            let req = tonic::Request::new(RangeRequest {
                key: key.clone().into_bytes(),
                range_end: if *prefix {
                    "".as_bytes().into()
                } else {
                    key.clone().into_bytes()
                },
                limit: *limit as i64,
                sort_order: 0,
                sort_target: 0,
                keys_only: *keys_only,
                count_only: *count_only,
            });
            let resp = client.range(req).await;
            match resp {
                Ok(res) => {
                    let range_response = res.into_inner();
                    println!("got {} items", range_response.count);
                    for (i, kv) in range_response.kvs.iter().enumerate() {
                        println!(
                            "item: {}\n{}\n{}",
                            i,
                            String::from_utf8(kv.key.clone())?,
                            String::from_utf8(kv.value.clone())?
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
            match response {
                Ok(res) => {
                    let res = res.into_inner();
                    if let Some(kv) = res.prev_kv {
                        let new_value = String::from_utf8_lossy(&kv.value);
                        println!("OK value changed:\n{}\n{}", new_value, val.clone());
                    }
                }
                Err(err) => {
                    log::error!("Err putting key {}: {}", &key, err)
                }
            }
        }
        Commands::Delete { key, prefix } => {
            let req = tonic::Request::new(DeleteRangeRequest {
                key: key.clone().into_bytes(),
                range_end: if *prefix {
                    "".as_bytes().into()
                } else {
                    key.clone().into_bytes()
                },
                prev_kv: true,
            });
            let response = client.delete_range(req).await;
            match response {
                Ok(res) => {
                    let res = res.into_inner();
                    for kv in res.prev_kvs {
                        println!(
                            "{}\n{}",
                            String::from_utf8(kv.key)?,
                            String::from_utf8(kv.value)?
                        );
                    }
                }
                Err(err) => {
                    log::error!("Err deleting key {}: {}", &key, err)
                }
            }
        }
    }

    Ok(())
}
