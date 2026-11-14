use std::path::PathBuf;
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

#[derive(Clone)]
pub struct Stash {
    tree: Arc<Mutex<TrieMap>>,
    tx: broadcast::Sender<Event>,
    snapshot: Option<PathBuf>,
}

impl Stash {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel::<Event>(100);
        Self {
            tree: Arc::new(Mutex::new(TrieMap::new())),
            tx,
            snapshot: None,
        }
    }

    /// Create a Stash persisted to `path`: the snapshot is loaded on start
    /// and rewritten atomically after every successful mutation.
    #[allow(dead_code)]
    pub fn with_snapshot(path: PathBuf) -> std::io::Result<Self> {
        let mut tree = TrieMap::new();
        if path.exists() {
            let kvs = utils::load_snapshot(&path)?;
            log::info!("loaded {} keys from snapshot {:?}", kvs.len(), path);
            for kv in kvs {
                tree.put(&kv.0, &kv.1);
            }
        } else if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let (tx, _rx) = broadcast::channel::<Event>(100);
        Ok(Self {
            tree: Arc::new(Mutex::new(tree)),
            tx,
            snapshot: Some(path),
        })
    }

    fn persist(&self, tree: &TrieMap) {
        if let Some(path) = &self.snapshot {
            let kvs = tree.traverse();
            if let Err(e) = utils::save_snapshot(path, &kvs) {
                log::error!("failed to write snapshot {:?}: {}", path, e);
            }
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
                return Err(Status::invalid_argument(e.to_string()));
            }
            Ok((k, v)) => (k, v),
        };

        if req.keys_only && req.count_only {
            return Err(Status::invalid_argument(
                "keys_only and count_only cannot be set together",
            ));
        }

        log::info!("GET: key '{}', range_end '{}'", key, range_end);

        let tree = self.tree.lock().await;
        let mut kvs = vec![];

        // etcd semantics:
        // - range_end empty: look up the single key
        // - range_end == "\0": all keys >= key
        // - otherwise: [key, range_end), upper bound exclusive
        let matches: Vec<_> = if range_end.is_empty() {
            tree.get(key.as_str()).into_iter().collect()
        } else if range_end == "\0" {
            tree.scan(&key, None)
        } else {
            tree.scan(&key, Some(&range_end))
        };

        let count = matches.len() as i64;
        let limit = if req.limit > 0 {
            Some(req.limit as usize)
        } else {
            None
        };
        let more = match limit {
            Some(l) => count > l as i64,
            None => false,
        };

        for item in matches.into_iter().take(limit.unwrap_or(usize::MAX)) {
            if req.count_only {
                continue;
            }
            let value = if req.keys_only { "" } else { item.1.as_str() };
            kvs.push(create_kv(item.0.as_str(), value));
        }

        log::info!("returned {} items, count {}", kvs.len(), count);

        Ok(Response::new(RangeResponse {
            kvs,
            more,
            count,
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
        self.persist(&tree);
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
                return Err(Status::invalid_argument(e.to_string()));
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

        // same range semantics as range(): empty range_end deletes the single
        // key, "\0" deletes all keys >= key, otherwise [key, range_end)
        let matches: Vec<_> = if range_end.is_empty() {
            tree.get(key.as_str()).into_iter().collect()
        } else if range_end == "\0" {
            tree.scan(&key, None)
        } else {
            tree.scan(&key, Some(&range_end))
        };

        let count = matches.len() as i64;
        for item in matches {
            tree.delete(item.0.as_str());
            prev_kvs.push(create_kv(item.0.as_str(), item.1.as_str()));
        }
        self.persist(&tree);

        for kv in &prev_kvs {
            self.send_delete(None, Some(kv.clone()))
        }

        Ok(Response::new(DeleteRangeResponse {
            deleted: count,
            prev_kvs: if req.prev_kv { prev_kvs } else { vec![] },
        }))
    }

    async fn txn(&self, _request: Request<TxnRequest>) -> Result<Response<TxnResponse>, Status> {
        Err(Status::unimplemented("transactions are not implemented"))
    }
}

#[tonic::async_trait]
impl Lease for Stash {
    async fn lease_grant(
        &self,
        _request: Request<LeaseGrantRequest>,
    ) -> Result<Response<LeaseGrantResponse>, Status> {
        Err(Status::unimplemented("leases are not implemented"))
    }

    async fn lease_revoke(
        &self,
        _request: Request<LeaseRevokeRequest>,
    ) -> Result<Response<LeaseRevokeResponse>, Status> {
        Err(Status::unimplemented("leases are not implemented"))
    }

    type LeaseKeepAliveStream =
    Pin<Box<dyn Stream<Item=Result<LeaseKeepAliveResponse, Status>> + Send>>;

    async fn lease_keep_alive(
        &self,
        _request: Request<Streaming<LeaseKeepAliveRequest>>,
    ) -> Result<Response<Self::LeaseKeepAliveStream>, Status> {
        Err(Status::unimplemented("leases are not implemented"))
    }

    async fn lease_time_to_live(
        &self,
        _request: Request<LeaseTimeToLiveRequest>,
    ) -> Result<Response<LeaseTimeToLiveResponse>, Status> {
        Err(Status::unimplemented("leases are not implemented"))
    }

    async fn lease_leases(
        &self,
        _request: Request<LeaseLeasesRequest>,
    ) -> Result<Response<LeaseLeasesResponse>, Status> {
        Err(Status::unimplemented("leases are not implemented"))
    }
}

#[tonic::async_trait]
impl Watch for Stash {
    type WatchStream = Pin<Box<dyn Stream<Item=Result<WatchResponse, Status>> + Send>>;

    async fn watch(
        &self,
        _request: Request<Streaming<WatchRequest>>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        Err(Status::unimplemented("watch is not implemented"))
    }
}

#[cfg(test)]
mod tests {
    use tonic::Request;

    use crate::proto::galadh::kv_server::Kv;
    use crate::proto::galadh::{DeleteRangeRequest, PutRequest, RangeRequest};

    use super::Stash;
    use super::utils::{load_snapshot, prefix_end};

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
                range_end: prefix_end(b"ab"),
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
        assert_eq!(resp.count, 3);
        assert!(!resp.more);
    }

    #[tokio::test]
    async fn test_range_prefix_limit_returns_more_flag() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ab".to_vec(),
                range_end: prefix_end(b"ab"),
                limit: 2,
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
        assert_eq!(returned, vec!["ab", "abc"]);
        assert_eq!(resp.count, 3);
        assert!(resp.more);
    }

    #[tokio::test]
    async fn test_range_count_only_returns_count_without_kvs() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ab".to_vec(),
                range_end: prefix_end(b"ab"),
                limit: 0,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: true,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.kvs.is_empty());
        assert_eq!(resp.count, 3);
    }

    #[tokio::test]
    async fn test_range_keys_only_and_count_only_is_invalid_argument() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ab".to_vec(),
                range_end: prefix_end(b"ab"),
                limit: 0,
                sort_order: 0,
                sort_target: 0,
                keys_only: true,
                count_only: true,
            }))
            .await;

        assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_range_exact_key_returns_one() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ac".to_vec(),
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

        assert_eq!(resp.count, 1);
        assert_eq!(
            String::from_utf8(resp.kvs[0].key.clone()).unwrap(),
            "ac"
        );
    }

    #[tokio::test]
    async fn test_range_exact_missing_key_returns_empty() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"nope".to_vec(),
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

        assert!(resp.kvs.is_empty());
        assert_eq!(resp.count, 0);
    }

    #[tokio::test]
    async fn test_range_upper_bound_is_exclusive() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ab".to_vec(),
                range_end: b"abd".to_vec(),
                limit: 0,
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
        assert_eq!(returned, vec!["ab", "abc"]);
    }

    #[tokio::test]
    async fn test_range_zero_range_end_returns_all_keys_from_key() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"ac".to_vec(),
                range_end: vec![0],
                limit: 0,
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
        assert_eq!(returned, vec!["ac", "zzz"]);
    }

    #[tokio::test]
    async fn test_delete_range_prefix_only_deletes_matching() {
        let stash = Stash::new();
        seed_stash(&stash).await;

        let del_resp = stash
            .delete_range(Request::new(DeleteRangeRequest {
                key: b"ab".to_vec(),
                range_end: prefix_end(b"ab"),
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
                range_end: prefix_end(b"a"),
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
                range_end: vec![],
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

        assert!(get_resp.kvs.is_empty());
    }

    #[tokio::test]
    async fn test_snapshot_roundtrip() {
        let dir = std::env::temp_dir().join(format!("galadh-test-{}", std::process::id()));
        let path = dir.join("galadh.snapshot");

        let stash = Stash::with_snapshot(path.clone()).unwrap();
        seed_stash(&stash).await;

        // the snapshot file now contains all 5 seeded keys
        let kvs = load_snapshot(&path).unwrap();
        assert_eq!(kvs.len(), 5);

        // a fresh Stash over the same file restores the data
        let stash2 = Stash::with_snapshot(path.clone()).unwrap();
        let resp = stash2
            .range(Request::new(RangeRequest {
                key: b"ab".to_vec(),
                range_end: prefix_end(b"ab"),
                limit: 0,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.count, 3);

        // deletions persist too
        stash2
            .delete_range(Request::new(DeleteRangeRequest {
                key: b"ab".to_vec(),
                range_end: prefix_end(b"ab"),
                prev_kv: false,
            }))
            .await
            .unwrap();
        let stash3 = Stash::with_snapshot(path).unwrap();
        let resp = stash3
            .range(Request::new(RangeRequest {
                key: b"a".to_vec(),
                range_end: prefix_end(b"a"),
                limit: 0,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.count, 1); // only "ac" remains

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_snapshot_rejects_bogus_length_prefix() {
        let dir = std::env::temp_dir().join(format!("galadh-test-evil-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("evil.snapshot");
        // key length prefix of u64::MAX followed by a few bytes: a corrupt
        // snapshot must be rejected with InvalidData, not panic the loader
        let mut buf = vec![0xFFu8; 8];
        buf.extend_from_slice(b"key");
        std::fs::write(&path, buf).unwrap();

        let err = load_snapshot(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_unimplemented_rpcs_return_status() {
        use crate::proto::galadh::lease_server::Lease;
        use crate::proto::galadh::{
            LeaseGrantRequest, LeaseRevokeRequest, LeaseTimeToLiveRequest, LeaseLeasesRequest,
            TxnRequest,
        };

        let stash = Stash::new();

        let resp = stash.txn(Request::new(TxnRequest::default())).await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);

        let resp = stash
            .lease_grant(Request::new(LeaseGrantRequest::default()))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);

        let resp = stash
            .lease_revoke(Request::new(LeaseRevokeRequest::default()))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);

        let resp = stash
            .lease_time_to_live(Request::new(LeaseTimeToLiveRequest::default()))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);

        let resp = stash
            .lease_leases(Request::new(LeaseLeasesRequest::default()))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);
    }

    /// Streaming rpcs cannot be called directly (tonic's `Streaming` type is
    /// not constructible outside the transport), so run a real in-process
    /// server and hit watch/lease_keep_alive over the wire.
    #[tokio::test]
    async fn test_streaming_rpcs_unimplemented_over_grpc() {
        use crate::proto::galadh::lease_server::LeaseServer;
        use crate::proto::galadh::lease_client::LeaseClient;
        use crate::proto::galadh::watch_server::WatchServer;
        use crate::proto::galadh::watch_client::WatchClient;
        use crate::proto::galadh::{LeaseKeepAliveRequest, WatchRequest};

        let stash = Stash::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(LeaseServer::new(stash.clone()))
                .add_service(WatchServer::new(stash))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        let mut lease = LeaseClient::connect(format!("http://{addr}")).await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            tx.send(LeaseKeepAliveRequest::default()).await.unwrap();
        });
        let resp = lease
            .lease_keep_alive(tokio_stream::wrappers::ReceiverStream::new(rx))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);

        let mut watch = WatchClient::connect(format!("http://{addr}")).await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            tx.send(WatchRequest::default()).await.unwrap();
        });
        let resp = watch
            .watch(tokio_stream::wrappers::ReceiverStream::new(rx))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);
    }
}
