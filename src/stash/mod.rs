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

use utils::create_kv;

use crate::proto::galadh::{
    DeleteRangeRequest, DeleteRangeResponse, LeaseGrantRequest, LeaseGrantResponse,
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

mod txn;
mod utils;

/// A registered lease: TTL in seconds, the moment it lapses and the keys
/// attached to it. Key -> lease association lives only here (the trie value
/// does not carry the lease id), so KeyValue.lease in responses is always 0.
#[derive(Debug)]
pub(crate) struct LeaseInfo {
    pub(crate) ttl: i64,
    pub(crate) expires_at: Instant,
    pub(crate) keys: HashSet<String>,
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
                self.send_event(Event {
                    r#type: 1, // DELETE
                    kv: Some(create_kv(&key, "")),
                    prev_kv: None,
                });
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

    /// Apply deferred side effects of a successful operation/transaction to
    /// the real state. Infallible: lease attach/detach cannot fail, persist
    /// failures are logged, broadcast send failures are logged.
    fn apply_commit_log(
        &self,
        tree: &mut TrieMap,
        leases: &mut HashMap<i64, LeaseInfo>,
        log: txn::CommitLog,
    ) {
        for (id, key) in &log.lease_attach {
            if let Some(info) = leases.get_mut(id) {
                info.keys.insert(key.clone());
            }
        }
        for key in &log.detached_keys {
            for info in leases.values_mut() {
                info.keys.remove(key);
            }
        }
        if log.mutated {
            self.persist(tree);
        }
        for event in log.events {
            self.send_event(event);
        }
    }

    fn send_event(&self, event: Event) {
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
        let tree = self.tree.lock().await;
        let resp = txn::range_core(&tree, &req)?;
        Ok(Response::new(resp))
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req = request.into_inner();

        log::info!(
            "PUT: key '{}', value '{}'",
            String::from_utf8_lossy(&req.key),
            String::from_utf8_lossy(&req.value)
        );

        let mut tree = self.tree.lock().await;
        let mut leases = self.leases.lock().await;

        let mut log = txn::CommitLog::default();
        let resp = txn::put_core(&mut tree, &leases, &req, &mut log)?;
        self.apply_commit_log(&mut tree, &mut leases, log);
        drop(tree);
        drop(leases);

        Ok(Response::new(resp))
    }

    async fn delete_range(
        &self,
        request: Request<DeleteRangeRequest>,
    ) -> Result<Response<DeleteRangeResponse>, Status> {
        let req = request.into_inner();

        log::info!(
            "DELETE: key '{}' range_end '{}' prev_kv '{}'",
            String::from_utf8_lossy(&req.key),
            String::from_utf8_lossy(&req.range_end),
            req.prev_kv
        );

        let mut tree = self.tree.lock().await;
        let mut log = txn::CommitLog::default();
        let resp = txn::delete_core(&mut tree, &req, &mut log)?;
        let mut leases = self.leases.lock().await;
        self.apply_commit_log(&mut tree, &mut leases, log);
        drop(leases);
        drop(tree);

        Ok(Response::new(resp))
    }

    async fn txn(&self, request: Request<TxnRequest>) -> Result<Response<TxnResponse>, Status> {
        let req = request.into_inner();

        // lock order tree -> leases everywhere; held for the whole txn so it
        // is atomic with respect to lease expiry and other writers.
        let mut tree = self.tree.lock().await;
        let mut leases = self.leases.lock().await;

        // run the transaction on a shadow clone and swap it in only on
        // success: no partial states, no undo journal.
        let mut shadow = tree.clone();
        let mut ctx = txn::TxnCtx {
            tree: &mut shadow,
            leases: &leases,
            log: txn::CommitLog::default(),
            depth: 0,
            ops_budget: txn::MAX_TXN_TOTAL_OPS,
        };
        let resp = txn::apply_txn(&mut ctx, &req)?;
        let txn_log = ctx.log;

        // commit: swap the shadow tree in, then apply deferred effects
        *tree = shadow;
        self.apply_commit_log(&mut tree, &mut leases, txn_log);
        drop(tree);
        drop(leases);

        Ok(Response::new(resp))
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
    use crate::proto::galadh::{DeleteRangeRequest, PutRequest, RangeRequest, TxnRequest};
    use crate::proto::galadh::{compare, request_op, response_op, Compare, RequestOp};

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
    async fn test_txn_empty_request_succeeds() {
        let stash = Stash::new();
        let resp = stash
            .txn(Request::new(TxnRequest::default()))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.succeeded);
        assert!(resp.responses.is_empty());
    }

    /// create-if-absent: `version EQUAL 0` is true only while the key does
    /// not exist, so the first txn puts and the second takes the failure
    /// branch — the etcd lock idiom.
    #[tokio::test]
    async fn test_txn_create_if_absent() {
        let make = |value: &'static str| TxnRequest {
            compare: vec![Compare {
                result: compare::CompareResult::Equal as i32,
                target: compare::CompareTarget::Version as i32,
                key: b"lock".to_vec(),
                target_union: Some(compare::TargetUnion::Version(0)),
                ..Default::default()
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(PutRequest {
                    key: b"lock".to_vec(),
                    value: value.as_bytes().to_vec(),
                    ..Default::default()
                })),
            }],
            failure: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(PutRequest {
                    key: b"failed".to_vec(),
                    value: b"1".to_vec(),
                    ..Default::default()
                })),
            }],
        };

        let stash = Stash::new();
        let first = stash.txn(Request::new(make("holder1"))).await.unwrap().into_inner();
        assert!(first.succeeded);

        let second = stash.txn(Request::new(make("holder2"))).await.unwrap().into_inner();
        assert!(!second.succeeded);

        // the success branch of the first txn and the failure branch of the
        // second are both visible
        let tree = stash.tree.lock().await;
        assert_eq!(tree.get("lock").unwrap().1, "holder1");
        assert_eq!(tree.get("failed").unwrap().1, "1");
    }

    #[tokio::test]
    async fn test_txn_cas() {
        let cas = |expected: &'static str, new: &'static str| TxnRequest {
            compare: vec![Compare {
                result: compare::CompareResult::Equal as i32,
                target: compare::CompareTarget::Value as i32,
                key: b"cas".to_vec(),
                target_union: Some(compare::TargetUnion::Value(expected.as_bytes().to_vec())),
                ..Default::default()
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(PutRequest {
                    key: b"cas".to_vec(),
                    value: new.as_bytes().to_vec(),
                    ..Default::default()
                })),
            }],
            failure: vec![],
        };

        let stash = Stash::new();
        stash
            .put(Request::new(PutRequest {
                key: b"cas".to_vec(),
                value: b"v1".to_vec(),
                ..Default::default()
            }))
            .await
            .unwrap();

        let resp = stash.txn(Request::new(cas("v1", "v2"))).await.unwrap().into_inner();
        assert!(resp.succeeded);

        // old value no longer matches -> guard false, value stays "v2"
        let resp = stash.txn(Request::new(cas("v1", "v3"))).await.unwrap().into_inner();
        assert!(!resp.succeeded);
        let tree = stash.tree.lock().await;
        assert_eq!(tree.get("cas").unwrap().1, "v2");
    }

    /// VALUE on a missing key is always false, even NOT_EQUAL — etcd
    /// semantics: absence is not "some other value".
    #[tokio::test]
    async fn test_txn_value_compare_on_missing_key_is_false() {
        let stash = Stash::new();
        let resp = stash
            .txn(Request::new(TxnRequest {
                compare: vec![Compare {
                    result: compare::CompareResult::NotEqual as i32,
                    target: compare::CompareTarget::Value as i32,
                    key: b"missing".to_vec(),
                    target_union: Some(compare::TargetUnion::Value(b"x".to_vec())),
                    ..Default::default()
                }],
                success: vec![RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"should-not-exist".to_vec(),
                        value: b"1".to_vec(),
                        ..Default::default()
                    })),
                }],
                failure: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.succeeded);
        let tree = stash.tree.lock().await;
        assert!(tree.get("should-not-exist").is_none());
    }

    /// Operations in the chosen branch see the writes of earlier operations
    /// of the same txn (read-your-writes), including a nested txn's guard.
    #[tokio::test]
    async fn test_txn_read_your_writes() {
        let stash = Stash::new();
        let req = TxnRequest {
            compare: vec![],
            success: vec![
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"k".to_vec(),
                        value: b"v".to_vec(),
                        ..Default::default()
                    })),
                },
                RequestOp {
                    request: Some(request_op::Request::RequestRange(RangeRequest {
                        key: b"k".to_vec(),
                        ..Default::default()
                    })),
                },
                RequestOp {
                    request: Some(request_op::Request::RequestTxn(TxnRequest {
                        compare: vec![Compare {
                            result: compare::CompareResult::Equal as i32,
                            target: compare::CompareTarget::Value as i32,
                            key: b"k".to_vec(),
                            target_union: Some(compare::TargetUnion::Value(b"v".to_vec())),
                            ..Default::default()
                        }],
                        success: vec![RequestOp {
                            request: Some(request_op::Request::RequestPut(PutRequest {
                                key: b"seen".to_vec(),
                                value: b"1".to_vec(),
                                ..Default::default()
                            })),
                        }],
                        failure: vec![],
                    })),
                },
            ],
            failure: vec![],
        };

        let resp = stash.txn(Request::new(req)).await.unwrap().into_inner();
        assert!(resp.succeeded);
        assert_eq!(resp.responses.len(), 3);

        // the range inside the txn already saw the new value
        let range = match resp.responses[1].response.as_ref().unwrap() {
            response_op::Response::ResponseRange(r) => r,
            other => panic!("expected range response, got {:?}", other),
        };
        assert_eq!(range.count, 1);
        assert_eq!(range.kvs[0].value, b"v".to_vec());

        // the nested txn's guard saw it too
        let tree = stash.tree.lock().await;
        assert!(tree.get("seen").is_some());
    }

    #[tokio::test]
    async fn test_txn_rejects_conflicting_writes() {
        let put = |key: &'static str| RequestOp {
            request: Some(request_op::Request::RequestPut(PutRequest {
                key: key.as_bytes().to_vec(),
                value: b"v".to_vec(),
                ..Default::default()
            })),
        };
        let del = |key: &'static str| RequestOp {
            request: Some(request_op::Request::RequestDeleteRange(DeleteRangeRequest {
                key: key.as_bytes().to_vec(),
                ..Default::default()
            })),
        };

        let stash = Stash::new();

        // duplicate put of one key
        let dup = TxnRequest {
            compare: vec![],
            success: vec![put("k"), put("k")],
            failure: vec![],
        };
        let err = stash.txn(Request::new(dup)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);

        // put inside the deleted range of the same txn ([a, b) covers "ab")
        let conflict = TxnRequest {
            compare: vec![],
            success: vec![
                RequestOp {
                    request: Some(request_op::Request::RequestDeleteRange(DeleteRangeRequest {
                        key: b"a".to_vec(),
                        range_end: b"b".to_vec(),
                        ..Default::default()
                    })),
                },
                put("ab"),
            ],
            failure: vec![],
        };
        let err = stash.txn(Request::new(conflict)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);

        // overlapping delete ranges are fine (both apply)
        let overlap = TxnRequest {
            compare: vec![],
            success: vec![del("a"), del("abc")], // [a,b) and [abc,abd) overlap on "abc"
            failure: vec![],
        };
        stash
            .put(Request::new(PutRequest {
                key: b"abc".to_vec(),
                value: b"v".to_vec(),
                ..Default::default()
            }))
            .await
            .unwrap();
        let resp = stash.txn(Request::new(overlap)).await.unwrap().into_inner();
        assert!(resp.succeeded);
        let tree = stash.tree.lock().await;
        assert!(tree.get("abc").is_none());
    }

    /// An error in the middle of the chosen branch aborts the whole txn:
    /// nothing is written, no events are emitted.
    #[tokio::test]
    async fn test_txn_error_midway_applies_nothing() {
        let stash = Stash::new();
        let mut rx = stash.tx.subscribe();

        let req = TxnRequest {
            compare: vec![],
            success: vec![
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"first".to_vec(),
                        value: b"1".to_vec(),
                        ..Default::default()
                    })),
                },
                // unknown lease -> failed_precondition
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"second".to_vec(),
                        value: b"2".to_vec(),
                        lease: 424242,
                        ..Default::default()
                    })),
                },
            ],
            failure: vec![],
        };

        let err = stash.txn(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::FailedPrecondition);

        let tree = stash.tree.lock().await;
        assert!(tree.get("first").is_none());
        drop(tree);

        // no events leaked from the aborted txn
        assert!(rx.try_recv().is_err());
    }

    /// A txn with mutations emits events in operation order after commit;
    /// a failed txn emits none.
    #[tokio::test]
    async fn test_txn_event_order() {
        let stash = Stash::new();
        let mut rx = stash.tx.subscribe();

        stash
            .put(Request::new(PutRequest {
                key: b"gone".to_vec(),
                value: b"v".to_vec(),
                ..Default::default()
            }))
            .await
            .unwrap();
        // drain the put event from setup
        rx.try_recv().unwrap();

        let req = TxnRequest {
            compare: vec![],
            success: vec![
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"a".to_vec(),
                        value: b"1".to_vec(),
                        ..Default::default()
                    })),
                },
                RequestOp {
                    request: Some(request_op::Request::RequestDeleteRange(DeleteRangeRequest {
                        key: b"gone".to_vec(),
                        ..Default::default()
                    })),
                },
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"c".to_vec(),
                        value: b"3".to_vec(),
                        ..Default::default()
                    })),
                },
            ],
            failure: vec![],
        };
        let resp = stash.txn(Request::new(req)).await.unwrap().into_inner();
        assert!(resp.succeeded);

        let ev1 = rx.try_recv().unwrap();
        assert_eq!(ev1.r#type, 0); // PUT a
        assert_eq!(ev1.kv.as_ref().unwrap().key, b"a".to_vec());

        let ev2 = rx.try_recv().unwrap();
        assert_eq!(ev2.r#type, 1); // DELETE gone
        assert_eq!(ev2.kv.as_ref().unwrap().key, b"gone".to_vec());
        assert_eq!(ev2.prev_kv.as_ref().unwrap().value, b"v".to_vec());

        let ev3 = rx.try_recv().unwrap();
        assert_eq!(ev3.r#type, 0); // PUT c
        assert!(rx.try_recv().is_err());

        // a failed txn emits no events
        let failing = TxnRequest {
            compare: vec![Compare {
                result: compare::CompareResult::Equal as i32,
                target: compare::CompareTarget::Version as i32,
                key: b"nonexistent".to_vec(),
                target_union: Some(compare::TargetUnion::Version(1)),
                ..Default::default()
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(PutRequest {
                    key: b"never".to_vec(),
                    value: b"1".to_vec(),
                    ..Default::default()
                })),
            }],
            failure: vec![],
        };
        let resp = stash.txn(Request::new(failing)).await.unwrap().into_inner();
        assert!(!resp.succeeded);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_txn_persists_snapshot_once() {
        let dir = std::env::temp_dir().join(format!("galadh-txn-{}", std::process::id()));
        let path = dir.join("galadh.snapshot");

        let persisted = Stash::with_snapshot(path.clone()).unwrap();

        let req = TxnRequest {
            compare: vec![],
            success: vec![
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"t1".to_vec(),
                        value: b"1".to_vec(),
                        ..Default::default()
                    })),
                },
                RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: b"t2".to_vec(),
                        value: b"2".to_vec(),
                        ..Default::default()
                    })),
                },
            ],
            failure: vec![],
        };
        persisted.txn(Request::new(req)).await.unwrap();

        let kvs = load_snapshot(&path).unwrap();
        let keys: Vec<_> = kvs.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["t1", "t2"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_txn_nesting_depth_limit() {
        let stash = Stash::new();

        // 5 levels of nesting work
        let mut inner = TxnRequest::default();
        for _ in 0..5 {
            inner = TxnRequest {
                compare: vec![],
                success: vec![RequestOp {
                    request: Some(request_op::Request::RequestTxn(inner)),
                }],
                failure: vec![],
            };
        }
        let resp = stash.txn(Request::new(inner)).await.unwrap().into_inner();
        assert!(resp.succeeded);

        // 33 levels are rejected instead of overflowing the stack
        let mut inner = TxnRequest::default();
        for _ in 0..33 {
            inner = TxnRequest {
                compare: vec![],
                success: vec![RequestOp {
                    request: Some(request_op::Request::RequestTxn(inner)),
                }],
                failure: vec![],
            };
        }
        let err = stash.txn(Request::new(inner)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("nesting too deep"));
    }

    #[tokio::test]
    async fn test_txn_too_many_ops_rejected() {
        let stash = Stash::new();
        let req = TxnRequest {
            compare: vec![],
            success: (0..129)
                .map(|i| RequestOp {
                    request: Some(request_op::Request::RequestPut(PutRequest {
                        key: format!("key-{i}").into_bytes(),
                        value: b"v".to_vec(),
                        ..Default::default()
                    })),
                })
                .collect(),
            failure: vec![],
        };
        let err = stash.txn(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("too many operations"));
    }

    /// Txn through the real gRPC stack: create-if-absent over the wire.
    #[tokio::test]
    async fn test_txn_over_grpc() {
        use crate::proto::galadh::kv_client::KvClient;
        use crate::proto::galadh::kv_server::KvServer;
        use crate::proto::galadh::{Compare, TxnRequest};

        let stash = Stash::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(KvServer::new(stash))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });

        let mut kv = KvClient::connect(format!("http://{addr}")).await.unwrap();
        let txn_req = TxnRequest {
            compare: vec![Compare {
                result: compare::CompareResult::Equal as i32,
                target: compare::CompareTarget::Version as i32,
                key: b"wire".to_vec(),
                target_union: Some(compare::TargetUnion::Version(0)),
                ..Default::default()
            }],
            success: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(PutRequest {
                    key: b"wire".to_vec(),
                    value: b"first".to_vec(),
                    ..Default::default()
                })),
            }],
            failure: vec![RequestOp {
                request: Some(request_op::Request::RequestPut(PutRequest {
                    key: b"wire-failed".to_vec(),
                    value: b"lost".to_vec(),
                    ..Default::default()
                })),
            }],
        };

        let resp = kv.txn(Request::new(txn_req.clone())).await.unwrap().into_inner();
        assert!(resp.succeeded);
        let resp = kv.txn(Request::new(txn_req)).await.unwrap().into_inner();
        assert!(!resp.succeeded);

        let got = kv
            .range(Request::new(RangeRequest {
                key: b"wire".to_vec(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(got.kvs[0].value, b"first".to_vec());
        // the failure branch of the second txn ran
        let got = kv
            .range(Request::new(RangeRequest {
                key: b"wire-failed".to_vec(),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(got.count, 1);
    }

    #[tokio::test]
    async fn test_put_rejects_empty_key() {
        let stash = Stash::new();
        let resp = stash
            .put(Request::new(PutRequest {
                key: vec![],
                value: b"v".to_vec(),
                ..Default::default()
            }))
            .await;
        assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);
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
