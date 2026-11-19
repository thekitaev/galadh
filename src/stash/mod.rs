use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::Stream;
use tokio::sync::{Mutex, broadcast, mpsc};
use tonic::{Request, Response, Status, Streaming};
use tokio_stream::wrappers::ReceiverStream;

use utils::{create_kv, read_key_val};

use crate::proto::galadh::{
    DeleteRangeRequest, DeleteRangeResponse, KeyValue, LeaseGrantRequest, LeaseGrantResponse,
    LeaseKeepAliveRequest, LeaseKeepAliveResponse, LeaseLeasesRequest, LeaseLeasesResponse,
    LeaseRevokeRequest, LeaseRevokeResponse, LeaseStatus, LeaseTimeToLiveRequest,
    LeaseTimeToLiveResponse, PutRequest, PutResponse, RangeRequest, RangeResponse, TxnRequest,
    TxnResponse, WatchCreateRequest, WatchRequest, WatchResponse,
};
use crate::proto::galadh::Event;
use crate::proto::galadh::kv_server::Kv;
use crate::proto::galadh::lease_server::Lease;
use crate::proto::galadh::watch_request;
use crate::proto::galadh::watch_server::Watch;
use crate::trie::TrieMap;

mod utils;

/// A registered lease: TTL in seconds, the moment it lapses and the keys
/// attached to it. Key -> lease association lives only here (the trie value
/// does not carry the lease id), so KeyValue.lease in responses is always 0.
#[derive(Debug)]
struct LeaseInfo {
    ttl: i64,
    expires_at: Instant,
    keys: HashSet<String>,
}

static LEASE_SEQ: AtomicI64 = AtomicI64::new(1);

fn next_lease_id() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    millis * 1000 + LEASE_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Stash {
    tree: Arc<Mutex<TrieMap>>,
    leases: Arc<Mutex<HashMap<i64, LeaseInfo>>>,
    tx: broadcast::Sender<Event>,
    snapshot: Option<PathBuf>,
}

impl Stash {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::build(TrieMap::new(), None)
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
        Ok(Self::build(tree, Some(path)))
    }

    fn build(tree: TrieMap, snapshot: Option<PathBuf>) -> Self {
        let stash = Self {
            tree: Arc::new(Mutex::new(tree)),
            leases: Arc::new(Mutex::new(HashMap::new())),
            tx: broadcast::channel::<Event>(256).0,
            snapshot,
        };
        stash.spawn_expiry_task();
        stash
    }

    /// Periodically revoke expired leases (deleting their keys). Must be
    /// called from within a tokio runtime.
    fn spawn_expiry_task(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let now = Instant::now();
                let expired: Vec<i64> = {
                    let leases = state.leases.lock().await;
                    leases
                        .iter()
                        .filter(|(_, info)| info.expires_at <= now)
                        .map(|(id, _)| *id)
                        .collect()
                };
                for id in expired {
                    let deleted = state.revoke_lease(id).await;
                    if deleted > 0 {
                        log::info!("expired lease {}: {} keys deleted", id, deleted);
                    }
                }
            }
        });
    }

    /// Remove the lease and delete all keys attached to it, emitting a
    /// delete event per key. Returns the number of keys deleted.
    async fn revoke_lease(&self, id: i64) -> usize {
        let keys: Vec<String> = {
            let mut leases = self.leases.lock().await;
            match leases.remove(&id) {
                Some(info) => info.keys.into_iter().collect(),
                None => return 0,
            }
        };
        let mut tree = self.tree.lock().await;
        let mut deleted = 0;
        for key in keys {
            if tree.get(&key).is_some() {
                tree.delete(&key);
                deleted += 1;
                self.send_delete(Some(create_kv(&key, "")), None);
            }
        }
        self.persist(&tree);
        log::info!("lease {} revoked, {} keys deleted", id, deleted);
        deleted
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

        if req.lease != 0 {
            let mut leases = self.leases.lock().await;
            match leases.get_mut(&req.lease) {
                Some(info) => {
                    info.keys.insert(key.clone());
                }
                None => {
                    return Err(Status::failed_precondition(format!(
                        "lease {} not found",
                        req.lease
                    )));
                }
            }
        }

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
        let deleted_keys: Vec<String> = matches.iter().map(|kv| kv.0.clone()).collect();
        for item in matches {
            tree.delete(item.0.as_str());
            prev_kvs.push(create_kv(item.0.as_str(), item.1.as_str()));
        }
        self.persist(&tree);

        // detach deleted keys from any lease that still references them
        if !deleted_keys.is_empty() {
            let mut leases = self.leases.lock().await;
            for info in leases.values_mut() {
                for key in &deleted_keys {
                    info.keys.remove(key);
                }
            }
        }

        for kv in &prev_kvs {
            // a delete event carries the deleted key itself (etcd semantics)
            let key = String::from_utf8_lossy(&kv.key);
            self.send_delete(Some(create_kv(&key, "")), Some(kv.clone()));
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
        request: Request<LeaseGrantRequest>,
    ) -> Result<Response<LeaseGrantResponse>, Status> {
        let req = request.into_inner();
        if req.ttl <= 0 {
            return Err(Status::invalid_argument("TTL must be positive"));
        }
        let id = if req.id != 0 { req.id } else { next_lease_id() };

        let mut leases = self.leases.lock().await;
        if leases.contains_key(&id) {
            return Err(Status::failed_precondition(format!(
                "lease {} already exists",
                id
            )));
        }
        leases.insert(
            id,
            LeaseInfo {
                ttl: req.ttl,
                expires_at: Instant::now() + Duration::from_secs(req.ttl as u64),
                keys: HashSet::new(),
            },
        );
        drop(leases);

        log::info!("lease {} granted, ttl {}s", id, req.ttl);
        Ok(Response::new(LeaseGrantResponse {
            id,
            ttl: req.ttl,
            error: String::new(),
        }))
    }

    async fn lease_revoke(
        &self,
        request: Request<LeaseRevokeRequest>,
    ) -> Result<Response<LeaseRevokeResponse>, Status> {
        let req = request.into_inner();
        let deleted = self.revoke_lease(req.id).await;
        if deleted == 0 {
            log::info!("lease {} revoke: lease not found", req.id);
        }
        Ok(Response::new(LeaseRevokeResponse {}))
    }

    type LeaseKeepAliveStream =
    Pin<Box<dyn Stream<Item=Result<LeaseKeepAliveResponse, Status>> + Send>>;

    async fn lease_keep_alive(
        &self,
        request: Request<Streaming<LeaseKeepAliveRequest>>,
    ) -> Result<Response<Self::LeaseKeepAliveStream>, Status> {
        let mut inbound = request.into_inner();
        let (out_tx, out_rx) = mpsc::channel::<Result<LeaseKeepAliveResponse, Status>>(16);
        let state = self.clone();

        tokio::spawn(async move {
            while let Ok(Some(req)) = inbound.message().await {
                let ttl = {
                    let mut leases = state.leases.lock().await;
                    match leases.get_mut(&req.id) {
                        Some(info) => {
                            info.expires_at = Instant::now() + Duration::from_secs(info.ttl as u64);
                            info.ttl
                        }
                        None => -1, // unknown or expired lease
                    }
                };
                let resp = LeaseKeepAliveResponse { id: req.id, ttl };
                if out_tx.send(Ok(resp)).await.is_err() {
                    break; // client went away
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
    }

    async fn lease_time_to_live(
        &self,
        request: Request<LeaseTimeToLiveRequest>,
    ) -> Result<Response<LeaseTimeToLiveResponse>, Status> {
        let req = request.into_inner();
        let leases = self.leases.lock().await;
        let resp = match leases.get(&req.id) {
            Some(info) => {
                let ttl = info
                    .expires_at
                    .saturating_duration_since(Instant::now())
                    .as_secs() as i64;
                let keys = if req.keys {
                    info.keys.iter().map(|k| k.as_bytes().to_vec()).collect()
                } else {
                    vec![]
                };
                LeaseTimeToLiveResponse {
                    id: req.id,
                    ttl,
                    granted_ttl: info.ttl,
                    keys,
                }
            }
            None => LeaseTimeToLiveResponse {
                id: req.id,
                ttl: -1,
                granted_ttl: 0,
                keys: vec![],
            },
        };
        Ok(Response::new(resp))
    }

    async fn lease_leases(
        &self,
        _request: Request<LeaseLeasesRequest>,
    ) -> Result<Response<LeaseLeasesResponse>, Status> {
        let leases = self.leases.lock().await;
        let list = leases
            .keys()
            .map(|id| LeaseStatus { id: *id })
            .collect();
        Ok(Response::new(LeaseLeasesResponse { leases: list }))
    }
}

/// Per-stream watcher registration.
struct WatcherSpec {
    key: Vec<u8>,
    range_end: Vec<u8>,
    filters: Vec<i32>,
    prev_kv: bool,
}

impl WatcherSpec {
    /// etcd range semantics: empty range_end watches a single key,
    /// "\0" everything >= key, otherwise [key, range_end).
    fn matches(&self, key: &[u8]) -> bool {
        if self.range_end.is_empty() {
            return key == &self.key[..];
        }
        if self.range_end == b"\0" {
            return key >= &self.key[..];
        }
        key >= &self.key[..] && key < &self.range_end[..]
    }

    fn filter_allows(&self, event_type: i32) -> bool {
        // FilterType: 0 = NOPUT, 1 = NODELETE
        !(self.filters.contains(&0) && event_type == 0)
            && !(self.filters.contains(&1) && event_type == 1)
    }
}

#[tonic::async_trait]
impl Watch for Stash {
    type WatchStream = Pin<Box<dyn Stream<Item=Result<WatchResponse, Status>> + Send>>;

    async fn watch(
        &self,
        request: Request<Streaming<WatchRequest>>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let mut inbound = request.into_inner();
        let (out_tx, out_rx) = mpsc::channel::<Result<WatchResponse, Status>>(64);
        let mut events = self.tx.subscribe();

        tokio::spawn(async move {
            // watch_id -> spec, local to this stream
            let mut watchers: HashMap<i64, WatcherSpec> = HashMap::new();
            let mut next_id: i64 = 1;

            loop {
                tokio::select! {
                    req = inbound.message() => {
                        match req {
                            Ok(Some(wr)) => match wr.request_union {
                                Some(watch_request::RequestUnion::CreateRequest(create)) => {
                                    let id = if create.watch_id != 0 {
                                        create.watch_id
                                    } else {
                                        next_id
                                    };
                                    next_id = next_id.max(id) + 1;
                                    log::info!(
                                        "WATCH create id {} key '{}' range_end '{}'",
                                        id,
                                        String::from_utf8_lossy(&create.key),
                                        String::from_utf8_lossy(&create.range_end)
                                    );
                                    let WatchCreateRequest {
                                        key,
                                        range_end,
                                        filters,
                                        prev_kv,
                                        ..
                                    } = create;
                                    watchers.insert(
                                        id,
                                        WatcherSpec { key, range_end, filters, prev_kv },
                                    );
                                    let resp = WatchResponse {
                                        watch_id: id,
                                        created: true,
                                        ..Default::default()
                                    };
                                    if out_tx.send(Ok(resp)).await.is_err() {
                                        return;
                                    }
                                }
                                Some(watch_request::RequestUnion::CancelRequest(cancel)) => {
                                    log::info!("WATCH cancel id {}", cancel.watch_id);
                                    watchers.remove(&cancel.watch_id);
                                    let resp = WatchResponse {
                                        watch_id: cancel.watch_id,
                                        canceled: true,
                                        ..Default::default()
                                    };
                                    if out_tx.send(Ok(resp)).await.is_err() {
                                        return;
                                    }
                                }
                                _ => {} // progress request: no-op
                            },
                            Ok(None) => break, // client closed the stream
                            Err(e) => {
                                log::warn!("watch inbound stream error: {}", e);
                                break;
                            }
                        }
                    }
                    ev = events.recv() => {
                        match ev {
                            Ok(event) => {
                                let key: Vec<u8> = event
                                    .kv
                                    .as_ref()
                                    .map(|kv| kv.key.clone())
                                    .unwrap_or_default();
                                for (id, spec) in &watchers {
                                    if !spec.matches(&key) || !spec.filter_allows(event.r#type) {
                                        continue;
                                    }
                                    let event = if spec.prev_kv {
                                        event.clone()
                                    } else {
                                        Event {
                                            prev_kv: None,
                                            ..event.clone()
                                        }
                                    };
                                    let resp = WatchResponse {
                                        watch_id: *id,
                                        events: vec![event],
                                        ..Default::default()
                                    };
                                    if out_tx.send(Ok(resp)).await.is_err() {
                                        return; // client went away
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                log::warn!("watch stream lagged, {} events dropped", n);
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
            log::info!("watch stream closed");
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
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
    async fn test_txn_is_still_unimplemented() {
        use crate::proto::galadh::TxnRequest;

        let stash = Stash::new();
        let resp = stash.txn(Request::new(TxnRequest::default())).await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::Unimplemented);
    }

    #[tokio::test]
    async fn test_lease_grant_revoke_deletes_attached_keys() {
        use crate::proto::galadh::lease_server::Lease;
        use crate::proto::galadh::{LeaseGrantRequest, LeaseRevokeRequest};

        let stash = Stash::new();
        let lease = stash
            .lease_grant(Request::new(LeaseGrantRequest {
                ttl: 60,
                id: 777,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(lease.id, 777);

        // attach a key to the lease
        stash
            .put(Request::new(PutRequest {
                key: b"leased".to_vec(),
                value: b"v".to_vec(),
                lease: 777,
                prev_kv: false,
                ignore_value: false,
                ignore_lease: false,
            }))
            .await
            .unwrap();

        // revoke: the attached key must disappear
        stash
            .lease_revoke(Request::new(LeaseRevokeRequest { id: 777 }))
            .await
            .unwrap();

        let resp = stash
            .range(Request::new(RangeRequest {
                key: b"leased".to_vec(),
                range_end: vec![],
                limit: 0,
                sort_order: 0,
                sort_target: 0,
                keys_only: false,
                count_only: false,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.count, 0);
    }

    #[tokio::test]
    async fn test_put_with_unknown_lease_fails() {
        let stash = Stash::new();
        let resp = stash
            .put(Request::new(PutRequest {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
                lease: 42,
                prev_kv: false,
                ignore_value: false,
                ignore_lease: false,
            }))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn test_lease_time_to_live_lists_attached_keys() {
        use crate::proto::galadh::lease_server::Lease;
        use crate::proto::galadh::{LeaseGrantRequest, LeaseTimeToLiveRequest};

        let stash = Stash::new();
        let lease = stash
            .lease_grant(Request::new(LeaseGrantRequest {
                ttl: 60,
                id: 0, // server-assigned id
            }))
            .await
            .unwrap()
            .into_inner();
        assert_ne!(lease.id, 0);

        stash
            .put(Request::new(PutRequest {
                key: b"attached".to_vec(),
                value: b"v".to_vec(),
                lease: lease.id,
                prev_kv: false,
                ignore_value: false,
                ignore_lease: false,
            }))
            .await
            .unwrap();

        let ttl = stash
            .lease_time_to_live(Request::new(LeaseTimeToLiveRequest {
                id: lease.id,
                keys: true,
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(ttl.ttl > 0 && ttl.ttl <= 60);
        assert_eq!(ttl.keys, vec![b"attached".to_vec()]);

        // unknown lease reports ttl == -1
        let ttl = stash
            .lease_time_to_live(Request::new(LeaseTimeToLiveRequest {
                id: 12345,
                keys: false,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(ttl.ttl, -1);
    }

    /// A lease with ttl = 1s must expire and delete its keys within a few
    /// seconds via the background expiry task.
    #[tokio::test]
    async fn test_lease_expiry_deletes_keys() {
        use crate::proto::galadh::lease_server::Lease;
        use crate::proto::galadh::LeaseGrantRequest;

        let stash = Stash::new();
        let lease = stash
            .lease_grant(Request::new(LeaseGrantRequest {
                ttl: 1,
                id: 0,
            }))
            .await
            .unwrap()
            .into_inner();

        stash
            .put(Request::new(PutRequest {
                key: b"ephemeral".to_vec(),
                value: b"v".to_vec(),
                lease: lease.id,
                prev_kv: false,
                ignore_value: false,
                ignore_lease: false,
            }))
            .await
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let count = stash
                .range(Request::new(RangeRequest {
                    key: b"ephemeral".to_vec(),
                    range_end: vec![],
                    limit: 0,
                    sort_order: 0,
                    sort_target: 0,
                    keys_only: false,
                    count_only: false,
                }))
                .await
                .unwrap()
                .into_inner()
                .count;
            if count == 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "lease key was not expired within 5s"
            );
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    /// Watch/keepalive are streaming rpcs — test them over a real in-process
    /// gRPC server.
    #[tokio::test]
    async fn test_watch_and_lease_keepalive_over_grpc() {
        use crate::proto::galadh::kv_client::KvClient;
        use crate::proto::galadh::kv_server::KvServer;
        use crate::proto::galadh::lease_client::LeaseClient;
        use crate::proto::galadh::lease_server::LeaseServer;
        use crate::proto::galadh::watch_client::WatchClient;
        use crate::proto::galadh::watch_server::WatchServer;
        use crate::proto::galadh::watch_request::RequestUnion;
        use crate::proto::galadh::{
            LeaseGrantRequest, LeaseKeepAliveRequest, PutRequest, WatchCreateRequest, WatchRequest,
        };

        let stash = Stash::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(KvServer::new(stash.clone()))
                .add_service(LeaseServer::new(stash.clone()))
                .add_service(WatchServer::new(stash))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        // --- watch: prefix "foo", then put/delete two keys via Kv service
        let mut watch = WatchClient::connect(format!("http://{addr}")).await.unwrap();
        let (wtx, wrx) = tokio::sync::mpsc::channel::<WatchRequest>(8);
        let mut stream = watch
            .watch(tokio_stream::wrappers::ReceiverStream::new(wrx))
            .await
            .unwrap()
            .into_inner();

        wtx.send(WatchRequest {
            request_union: Some(RequestUnion::CreateRequest(WatchCreateRequest {
                key: b"foo".to_vec(),
                range_end: prefix_end(b"foo"),
                ..Default::default()
            })),
        })
        .await
        .unwrap();

        let created = tokio::time::timeout(std::time::Duration::from_secs(2), stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(created.created);
        let watch_id = created.watch_id;

        let mut kv = KvClient::connect(format!("http://{addr}")).await.unwrap();
        kv.put(Request::new(PutRequest {
            key: b"foo".to_vec(),
            value: b"bar".to_vec(),
            ..Default::default()
        }))
        .await
        .unwrap();

        let put_event = tokio::time::timeout(std::time::Duration::from_secs(2), stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(put_event.watch_id, watch_id);
        assert_eq!(put_event.events.len(), 1);
        assert_eq!(put_event.events[0].r#type, 0); // PUT
        assert_eq!(
            put_event.events[0].kv.as_ref().unwrap().key,
            b"foo".to_vec()
        );

        // key outside the watched prefix must NOT produce an event
        kv.put(Request::new(PutRequest {
            key: b"other".to_vec(),
            value: b"x".to_vec(),
            ..Default::default()
        }))
        .await
        .unwrap();

        // --- lease keepalive over the wire
        let mut lease = LeaseClient::connect(format!("http://{addr}")).await.unwrap();
        let granted = lease
            .lease_grant(LeaseGrantRequest { ttl: 60, id: 0 })
            .await
            .unwrap()
            .into_inner();

        let (ltx, lrx) = tokio::sync::mpsc::channel::<LeaseKeepAliveRequest>(8);
        let mut keepalive = lease
            .lease_keep_alive(tokio_stream::wrappers::ReceiverStream::new(lrx))
            .await
            .unwrap()
            .into_inner();
        ltx.send(LeaseKeepAliveRequest { id: granted.id })
            .await
            .unwrap();
        let ka = tokio::time::timeout(std::time::Duration::from_secs(2), keepalive.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(ka.id, granted.id);
        assert_eq!(ka.ttl, 60);

        // unknown lease -> ttl == -1
        ltx.send(LeaseKeepAliveRequest { id: 999999 })
            .await
            .unwrap();
        let ka = tokio::time::timeout(std::time::Duration::from_secs(2), keepalive.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(ka.ttl, -1);

        // --- cancel the watcher
        wtx.send(WatchRequest {
            request_union: Some(RequestUnion::CancelRequest(
                crate::proto::galadh::WatchCancelRequest { watch_id },
            )),
        })
        .await
        .unwrap();
        let canceled = tokio::time::timeout(std::time::Duration::from_secs(2), stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(canceled.canceled);
    }
}
