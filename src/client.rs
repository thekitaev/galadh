mod proto;

use crate::proto::galadh::kv_client::KvClient;
use crate::proto::galadh::{
    compare, request_op, Compare, DeleteRangeRequest, PutRequest, RangeRequest, RequestOp,
    TxnRequest,
};

use std::env;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    /// gRPC endpoint of the galadh server
    #[clap(long, default_value = "http://127.0.0.1:2379")]
    addr: String,
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
    /// put the value only if the key does not exist (txn under the hood)
    PutIfAbsent { key: String, val: String },
    /// compare-and-set: replace the value only if it currently equals EXPECTED
    Cas {
        key: String,
        expected: String,
        new: String,
    },
}

/// etcd-style range end for a prefix scan: increment the last byte,
/// carrying over 0xff bytes. A key of all 0xff has no successor,
/// so "\0" (all keys >= key) is used instead.
fn prefix_end(key: &str) -> Vec<u8> {
    let mut end = key.as_bytes().to_vec();
    for i in (0..end.len()).rev() {
        if end[i] < 0xff {
            end[i] += 1;
            return end;
        }
    }
    vec![0]
}

/// TxnRequest: put the value only while the key is absent (version == 0).
fn txn_create_if_absent(key: &str, val: &str) -> TxnRequest {
    TxnRequest {
        compare: vec![Compare {
            result: compare::CompareResult::Equal as i32,
            target: compare::CompareTarget::Version as i32,
            key: key.as_bytes().to_vec(),
            target_union: Some(compare::TargetUnion::Version(0)),
            ..Default::default()
        }],
        success: vec![RequestOp {
            request: Some(request_op::Request::RequestPut(PutRequest {
                key: key.as_bytes().to_vec(),
                value: val.as_bytes().to_vec(),
                ..Default::default()
            })),
        }],
        failure: vec![],
    }
}

/// TxnRequest: replace the value only if it currently equals `expected`.
fn txn_cas(key: &str, expected: &str, new: &str) -> TxnRequest {
    TxnRequest {
        compare: vec![Compare {
            result: compare::CompareResult::Equal as i32,
            target: compare::CompareTarget::Value as i32,
            key: key.as_bytes().to_vec(),
            target_union: Some(compare::TargetUnion::Value(expected.as_bytes().to_vec())),
            ..Default::default()
        }],
        success: vec![RequestOp {
            request: Some(request_op::Request::RequestPut(PutRequest {
                key: key.as_bytes().to_vec(),
                value: new.as_bytes().to_vec(),
                ..Default::default()
            })),
        }],
        failure: vec![],
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    let mut client = KvClient::connect(cli.addr.clone()).await?;
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
                    prefix_end(key)
                } else {
                    // empty range_end: exact key lookup (etcd semantics)
                    vec![]
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
                    log::error!("Err getting key {}: {}", key, err)
                }
            }
        }
        Commands::Put { key, val } => {
            let req = tonic::Request::new(PutRequest {
                key: key.clone().into_bytes(),
                value: val.clone().into_bytes(),
                lease: 0,
                prev_kv: true,
                ignore_value: false,
                ignore_lease: false,
            });
            let response = client.put(req).await;
            match response {
                Ok(res) => {
                    let res = res.into_inner();
                    match res.prev_kv {
                        Some(kv) => println!(
                            "OK, replaced previous value:\n{}",
                            String::from_utf8_lossy(&kv.value)
                        ),
                        None => println!("OK, key created:\n{}\n{}", key, val),
                    }
                }
                Err(err) => {
                    log::error!("Err putting key {}: {}", key, err)
                }
            }
        }
        Commands::PutIfAbsent { key, val } => {
            let req = txn_create_if_absent(key, val);
            match client.txn(tonic::Request::new(req)).await {
                Ok(res) => {
                    if res.into_inner().succeeded {
                        println!("OK, key created:\n{}\n{}", key, val);
                    } else {
                        eprintln!("FAILED: key already exists");
                        std::process::exit(1);
                    }
                }
                Err(err) => {
                    log::error!("Err put-if-absent {}: {}", key, err);
                    std::process::exit(1);
                }
            }
        }
        Commands::Cas {
            key,
            expected,
            new,
        } => {
            let req = txn_cas(key, expected, new);
            match client.txn(tonic::Request::new(req)).await {
                Ok(res) => {
                    if res.into_inner().succeeded {
                        println!("OK, value replaced:\n{} -> {}", expected, new);
                    } else {
                        eprintln!("FAILED: current value does not match {}", expected);
                        std::process::exit(1);
                    }
                }
                Err(err) => {
                    log::error!("Err cas {}: {}", key, err);
                    std::process::exit(1);
                }
            }
        }
        Commands::Delete { key, prefix } => {
            let req = tonic::Request::new(DeleteRangeRequest {
                key: key.clone().into_bytes(),
                range_end: if *prefix {
                    prefix_end(key)
                } else {
                    // empty range_end: delete the single key (etcd semantics)
                    vec![]
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
                    log::error!("Err deleting key {}: {}", key, err)
                }
            }
        }
    }

    Ok(())
}
