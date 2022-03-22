use prefix_tree::PrefixMap;
use tokio::sync::broadcast;
use tokio::sync::{Mutex, MutexGuard};
use tonic::{Request, Response, Status, Streaming};

use crate::proto::galadh::kv_server::Kv;
use crate::proto::galadh::lease_server::Lease;
use crate::proto::galadh::watch_request::RequestUnion;
use crate::proto::galadh::watch_server::Watch;
use crate::proto::galadh::Event;
use crate::proto::galadh::{
    DeleteRangeRequest, DeleteRangeResponse, KeyValue, LeaseGrantRequest, LeaseGrantResponse,
    LeaseKeepAliveRequest, LeaseKeepAliveResponse, LeaseLeasesRequest, LeaseLeasesResponse,
    LeaseRevokeRequest, LeaseRevokeResponse, LeaseTimeToLiveRequest, LeaseTimeToLiveResponse,
    PutRequest, PutResponse, RangeRequest, RangeResponse, TxnRequest, TxnResponse, WatchRequest,
    WatchResponse,
};
mod utils;

use utils::{create_kv, read_key_val};

use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;

type Tree = PrefixMap<u8, String>;

pub struct Stash {
    tree: Arc<Mutex<Tree>>,
    tx: broadcast::Sender<Event>,
    rx: broadcast::Receiver<Event>,
}

impl Stash {
    pub(crate) fn new() -> Self {
        let (tx, rx) = broadcast::channel::<Event>(100);
        Self {
            tree: Arc::new(Mutex::new(PrefixMap::new())),
            rx,
            tx,
        }
    }

    fn send_put(&self, new_kv: Option<KeyValue>, old_kv: Option<KeyValue>) {
        self.send_event(0, new_kv, old_kv)
    }

    fn send_delete(&self, new_kv: Option<KeyValue>, old_kv: Option<KeyValue>) {
        self.send_event(1, new_kv, old_kv)
    }

    fn send_event(&self, event_type: i32, new_kv: Option<KeyValue>, old_kv: Option<KeyValue>) {
        let event = Event {
            r#type: event_type,
            kv: new_kv,
            prev_kv: old_kv,
        };

        let send_result = self.tx.send(event.clone());
        if let Err(e) = send_result {
            log::error!("err publishing event '{:?}': {}", event, e)
        }
    }

    fn find_range(
        &self,
        tree: &MutexGuard<Tree>,
        key: &str,
        range_end: &str,
    ) -> Vec<(String, String)> {
        let select_mode = match range_end {
            key => SelectMode::One,
            "" => SelectMode::Infinite,
            _ => SelectMode::Range,
        };
        let mut res = vec![];
        let key = key.to_string();

        for (k, v) in tree.iter() {
            let item_key = String::from_utf8_lossy(&k).to_string();
            if item_key.len() < key.len() {
                continue;
            }
            if select_mode == SelectMode::One && item_key == key {
                return vec![(item_key, v.clone())];
            }

            if item_key >= key {
                res.push((item_key.clone(), v.clone()))
            }

            if select_mode != SelectMode::Infinite && item_key.len() > range_end.len() {
                break;
            }
        }
        res
    }
}

#[derive(PartialEq, Eq)]
enum SelectMode {
    One,
    Range,
    Infinite,
}

#[tonic::async_trait]
impl Kv for Stash {
    async fn range(
        &self,
        request: Request<RangeRequest>,
    ) -> Result<Response<RangeResponse>, Status> {
        let req = request.into_inner();

        let (key, range_end) = match read_key_val(req.key, req.range_end) {
            Err(e) => {
                log::error!("error reading key-range_end: {}", e);
                return Err(Status::unknown(e.to_string()));
            }
            Ok((k, v)) => (k, v),
        };

        log::info!("GET: key '{}', range_end '{}'", key, range_end);

        let tree = self.tree.lock().await;
        let mut count = 0;

        let mut kvs = vec![];
        let matches: Vec<_> = self.find_range(&tree, key.as_str(), range_end.as_str());

        for item in &matches {
            let (k, v) = item;

            let kv: Option<KeyValue> = match (req.keys_only, req.count_only) {
                (true, true) => return Err(Status::unknown("bad request")),
                (true, false) => Some(create_kv(&k, "", 0)),
                (false, true) => None,
                (false, false) => Some(create_kv(&k, &v, 0)),
            };
            if let Some(kv) = kv {
                kvs.push(kv)
            };
            count += 1
        }

        let total = matches.len() as i64;

        Ok(Response::new(RangeResponse {
            kvs,
            more: total > count,
            count: total,
        }))
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req = request.into_inner();

        let (key, val) = match read_key_val(req.key, req.value) {
            Err(e) => {
                log::error!("error reading key-value: {}", e);
                return Err(Status::unknown(e.to_string()));
            }
            Ok((k, v)) => (k, v),
        };

        log::info!("PUT: key '{}', value '{}'", key, val);

        let mut tree = self.tree.lock().await;

        let old_val = tree.insert(&key, val.clone());
        let prev_kv = match (old_val, req.prev_kv) {
            (Some(v), true) => Some(create_kv(&key, &v, 0)),
            (_, _) => None,
        };

        self.send_put(
            Some(create_kv(key.as_str(), val.as_str(), 0)),
            prev_kv.clone(),
        );

        Ok(Response::new(PutResponse {
            prev_kv: if req.prev_kv { prev_kv } else { None },
        }))
    }

    async fn delete_range(
        &self,
        request: Request<DeleteRangeRequest>,
    ) -> Result<Response<DeleteRangeResponse>, Status> {
        let req = request.into_inner();

        let (key, range_end) = match read_key_val(req.key, req.range_end) {
            Err(e) => {
                log::error!("error reading key-range_end: {}", e);
                return Err(Status::unknown(e.to_string()));
            }
            Ok((k, v)) => (k, v),
        };

        log::info!(
            "DELETE: key '{}' range_end '{}' prev_kv '{}'",
            key,
            range_end,
            req.prev_kv
        );

        let mut tree = self.tree.lock().await;
        let matches: Vec<_> = self.find_range(&tree, key.as_str(), range_end.as_str());

        let mut prev_kvs = vec![];
        let mut count = 0;

        for item in matches {
            let (k, v) = item;

            tree.remove(&k);
            count += 1;
            prev_kvs.push(create_kv(&k, &v, 0));
        }

        for kv in &prev_kvs {
            self.send_delete(None, Some(kv.clone()))
        }

        Ok(Response::new(DeleteRangeResponse {
            deleted: count,
            prev_kvs: if req.prev_kv { prev_kvs } else { vec![] },
        }))
    }

    async fn txn(&self, request: Request<TxnRequest>) -> Result<Response<TxnResponse>, Status> {
        todo!()
    }
}

#[tonic::async_trait]
impl Lease for Stash {
    async fn lease_grant(
        &self,
        request: Request<LeaseGrantRequest>,
    ) -> Result<Response<LeaseGrantResponse>, Status> {
        todo!()
    }

    async fn lease_revoke(
        &self,
        request: Request<LeaseRevokeRequest>,
    ) -> Result<Response<LeaseRevokeResponse>, Status> {
        todo!()
    }

    type LeaseKeepAliveStream =
        Pin<Box<dyn Stream<Item = Result<LeaseKeepAliveResponse, Status>> + Send>>;

    async fn lease_keep_alive(
        &self,
        request: Request<Streaming<LeaseKeepAliveRequest>>,
    ) -> Result<Response<Self::LeaseKeepAliveStream>, Status> {
        todo!()
    }

    async fn lease_time_to_live(
        &self,
        request: Request<LeaseTimeToLiveRequest>,
    ) -> Result<Response<LeaseTimeToLiveResponse>, Status> {
        todo!()
    }

    async fn lease_leases(
        &self,
        request: Request<LeaseLeasesRequest>,
    ) -> Result<Response<LeaseLeasesResponse>, Status> {
        todo!()
    }
}

#[tonic::async_trait]
impl Watch for Stash {
    type WatchStream = Pin<Box<dyn Stream<Item = Result<WatchResponse, Status>> + Send>>;

    async fn watch(
        &self,
        request: Request<Streaming<WatchRequest>>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let mut req = request.into_inner();

        loop {
            let msg = req.message().await;
            if let Ok(msg) = msg {
                if let Some(msg) = msg {
                    if let Some(ru) = msg.request_union {
                        match ru {
                            RequestUnion::CreateRequest(r) => {
                                let (key, range_end) = match read_key_val(r.key, r.range_end) {
                                    Err(e) => {
                                        log::error!("error reading key-range_end: {}", e);
                                        return Err(Status::unknown(e.to_string()));
                                    }
                                    Ok((k, v)) => (k, v),
                                };
                            }
                            RequestUnion::CancelRequest(r) => todo!(),
                            RequestUnion::ProgressRequest(r) => todo!(),
                        };
                    }
                }
            }
        }
        let rx = self.tx.subscribe();
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use prefix_tree::PrefixMap;

    use crate::stash::Stash;

    #[test]
    fn test_tree_sorted() {
        let mut tree = PrefixMap::new();
        let mut keys = vec!["abc", "def", "abb", "aba", "zxy"];
        for k in &keys {
            tree.insert(*k, "value".to_string());
        }
        keys.sort();

        for (i, k) in tree.keys().enumerate() {
            assert_eq!(String::from_utf8_lossy(&k), keys[i].to_string())
        }
    }
}
