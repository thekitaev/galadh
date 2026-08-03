use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tonic::{Request, Response, Status, Streaming};

use utils::{create_kv, read_key_val};

use crate::proto::galadh::{
    DeleteRangeRequest, DeleteRangeResponse, KeyValue, LeaseGrantRequest, LeaseGrantResponse,
    LeaseKeepAliveRequest, LeaseKeepAliveResponse, LeaseLeasesRequest, LeaseLeasesResponse,
    LeaseRevokeRequest, LeaseRevokeResponse, LeaseTimeToLiveRequest, LeaseTimeToLiveResponse,
    PutRequest, PutResponse, RangeRequest, RangeResponse, TxnRequest, TxnResponse, WatchRequest,
    WatchResponse,
};
use crate::proto::galadh::Event;
use crate::proto::galadh::kv_server::Kv;
use crate::proto::galadh::lease_server::Lease;
use crate::proto::galadh::watch_server::Watch;
use crate::trie::TrieMap;

mod utils;

pub struct Stash {
    tree: Arc<Mutex<TrieMap>>,
    tx: broadcast::Sender<Event>,
    #[allow(dead_code)]
    rx: broadcast::Receiver<Event>,
}

impl Stash {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let (tx, rx) = broadcast::channel::<Event>(100);
        Self {
            tree: Arc::new(Mutex::new(TrieMap::new())),
            tx,
            rx,
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
        let mut total = 0;
        let mut kvs = vec![];

        if key == range_end {
            let res = tree.get(key.as_str());
            if let Some(res) = res {
                kvs.push(create_kv(res.0.as_str(), res.1.as_str()));
                total = 1
            }
        } else {
            let matches = tree.traverse();
            log::info!("found {}", matches.len());
            for item in &matches {
                if item.0 < key
                    || (!range_end.is_empty() && item.0 > range_end)
                    || (range_end.is_empty() && !item.0.starts_with(&key))
                {
                    log::info!("skipping");
                    continue
                }
                let kv: Option<KeyValue> = match (req.keys_only, req.count_only) {
                    (true, true) => return Err(Status::unknown("bad request")),
                    (true, false) => Some(create_kv(item.0.as_str(), "")),
                    (false, true) => None,
                    (false, false) => Some(create_kv(item.0.as_str(), item.1.as_str())),
                };
                if let Some(kv) = kv {
                    kvs.push(kv)
                };
                count += 1;
            }
            total = matches.len() as i64;
        }

        log::info!("returned {} items", kvs.len());

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

        let old_val = tree.put(&key, val.as_str());
        let prev_kv = match (old_val, req.prev_kv) {
            (Some(v), true) => Some(create_kv(&v.0, &v.1)),
            (_, _) => None,
        };

        self.send_put(
            Some(create_kv(key.as_str(), val.as_str())),
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

        let mut prev_kvs = vec![];
        let mut count: i64 = 0;

        if key == range_end {
            let old = tree.get(key.as_str());
            if let Some(kv) = old {
                tree.delete(key.as_str());
                count = 1;
                prev_kvs.push(create_kv(kv.0.as_str(), kv.1.as_str()));
            }
        } else {
            let matches = tree.traverse();
            log::info!("found {} candidates to delete", matches.len());
            for item in matches {
                if item.0 < key
                    || (!range_end.is_empty() && item.0 > range_end)
                    || (range_end.is_empty() && !item.0.starts_with(&key))
                {
                    continue;
                }
                tree.delete(item.0.as_str());
                count += 1;
                prev_kvs.push(create_kv(item.0.as_str(), item.1.as_str()));
            }
        }

        for kv in &prev_kvs {
            self.send_delete(None, Some(kv.clone()))
        }

        Ok(Response::new(DeleteRangeResponse {
            deleted: count,
            prev_kvs: if req.prev_kv { prev_kvs } else { vec![] },
        }))
    }

    async fn txn(&self, _request: Request<TxnRequest>) -> Result<Response<TxnResponse>, Status> {
        todo!()
    }
}

#[tonic::async_trait]
impl Lease for Stash {
    async fn lease_grant(
        &self,
        _request: Request<LeaseGrantRequest>,
    ) -> Result<Response<LeaseGrantResponse>, Status> {
        todo!()
    }

    async fn lease_revoke(
        &self,
        _request: Request<LeaseRevokeRequest>,
    ) -> Result<Response<LeaseRevokeResponse>, Status> {
        todo!()
    }

    type LeaseKeepAliveStream =
    Pin<Box<dyn Stream<Item=Result<LeaseKeepAliveResponse, Status>> + Send>>;

    async fn lease_keep_alive(
        &self,
        _request: Request<Streaming<LeaseKeepAliveRequest>>,
    ) -> Result<Response<Self::LeaseKeepAliveStream>, Status> {
        todo!()
    }

    async fn lease_time_to_live(
        &self,
        _request: Request<LeaseTimeToLiveRequest>,
    ) -> Result<Response<LeaseTimeToLiveResponse>, Status> {
        todo!()
    }

    async fn lease_leases(
        &self,
        _request: Request<LeaseLeasesRequest>,
    ) -> Result<Response<LeaseLeasesResponse>, Status> {
        todo!()
    }
}

#[tonic::async_trait]
impl Watch for Stash {
    type WatchStream = Pin<Box<dyn Stream<Item=Result<WatchResponse, Status>> + Send>>;

    async fn watch(
        &self,
        _request: Request<Streaming<WatchRequest>>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use tonic::Request;

    use crate::proto::galadh::kv_server::Kv;
    use crate::proto::galadh::{DeleteRangeRequest, PutRequest, RangeRequest};

    use super::Stash;

    async fn seed_stash(stash: &Stash) {
        let keys = vec!["ab", "abc", "abd", "ac", "zzz"];
        for k in keys {
            stash
                .put(Request::new(PutRequest {
                    key: k.into(),
                    value: format!("v_{k}").into_bytes(),
                    lease: 0,
                    prev_kv: false,
                    ignore_value: false,
                    ignore_lease: false,
                }))
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn test_range_prefix_only_returns_matching_keys() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ab".to_vec(),
                range_end: vec![],
                limit: 100,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();

        let returned: Vec<String> = resp
            .kvs
            .iter()
            .map(|kv| String::from_utf8(kv.key.clone()).unwrap())
            .collect();
        assert_eq!(returned, vec!["ab", "abc", "abd"]);
    }

    #[tokio::test]
    async fn test_range_exact_key_returns_one() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ac".to_vec(),
                range_end: b"ac".to_vec(),
                limit: 100,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.kvs.len(), 1);
        assert_eq!(
            String::from_utf8(resp.kvs[0].key.clone()).unwrap(),
            "ac"
        );
    }

    #[tokio::test]
    async fn test_delete_range_prefix_only_deletes_matching() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let del_resp = stash
            .delete_range(Request::new(DeleteRangeRequest {
                key: b"ab".to_vec(),
                range_end: vec![],
                prev_kv: true,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(del_resp.deleted, 3);
        let deleted: Vec<String> = del_resp
            .prev_kvs
            .iter()
            .map(|kv| String::from_utf8(kv.key.clone()).unwrap())
            .collect();
        assert_eq!(deleted, vec!["ab", "abc", "abd"]);

        // surviving keys: "ac", "zzz"
        let get_resp = stash
            .range(Request::new(RangeRequest {
                key: b"a".to_vec(),
                range_end: vec![],
                limit: 100,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();

        let remaining: Vec<String> = get_resp
            .kvs
            .iter()
            .map(|kv| String::from_utf8(kv.key.clone()).unwrap())
            .collect();
        assert_eq!(remaining, vec!["ac"]);
    }

    #[tokio::test]
    async fn test_delete_range_exact_key_deletes_one() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let del_resp = stash
            .delete_range(Request::new(DeleteRangeRequest {
                key: b"zzz".to_vec(),
                range_end: b"zzz".to_vec(),
                prev_kv: true,
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(del_resp.deleted, 1);
        assert_eq!(
            String::from_utf8(del_resp.prev_kvs[0].key.clone()).unwrap(),
            "zzz"
        );

        // verify it's gone
        let get_resp = stash
            .range(Request::new(RangeRequest {
                key: b"zzz".to_vec(),
                range_end: b"zzz".to_vec(),
                limit: 100,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(get_resp.kvs.is_empty());
    }
}
