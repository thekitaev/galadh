//! Txn engine and the operation "cores" shared by the standalone RPCs and
//! transactions. Cores mutate a `TrieMap` directly and defer side effects
//! (lease attach/detach, watch events, persistence) to a `CommitLog` that
//! the caller applies only after a successful commit — this is what makes
//! a txn atomic: it runs on a shadow clone of the tree, and the clone is
//! swapped in only if every operation succeeded.

// tonic::Status is the error type of the whole gRPC layer; boxing it in
// internal helpers would buy nothing.
#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};

use tonic::Status;

use crate::proto::galadh::{
    compare, compare::CompareResult, compare::CompareTarget, request_op, response_op, Compare,
    DeleteRangeRequest, DeleteRangeResponse, Event, PutRequest, PutResponse, RangeRequest,
    RangeResponse, RequestOp, ResponseOp, TxnRequest, TxnResponse,
};
use crate::trie::KVPair;
use crate::trie::TrieMap;

use super::LeaseInfo;
use super::utils::create_kv;

/// Max operations per list (compare / success / failure) at one txn level,
/// mirroring etcd's maxTxnOps.
pub(crate) const MAX_TXN_OPS: usize = 128;
/// Max nesting depth of nested txns. Without it, a deep-enough legal gRPC
/// message would overflow a worker's stack and abort the whole process.
pub(crate) const MAX_TXN_DEPTH: usize = 32;
/// Total operation budget across all nesting levels.
pub(crate) const MAX_TXN_TOTAL_OPS: usize = 1024;

/// Side effects deferred until the transaction commits.
#[derive(Default)]
pub(crate) struct CommitLog {
    /// watch events in operation order, sent after commit
    pub events: Vec<Event>,
    /// (lease id, key) pairs requested by puts with lease != 0
    pub lease_attach: Vec<(i64, String)>,
    /// keys removed by delete_range, to detach from their leases
    pub detached_keys: Vec<String>,
    /// whether any put/delete mutated the tree
    pub mutated: bool,
}

/// Execution context over a (shadow) tree.
pub(crate) struct TxnCtx<'a> {
    pub tree: &'a mut TrieMap,
    /// read-only: put_core checks lease existence, compare LEASE resolves key->lease
    pub leases: &'a HashMap<i64, LeaseInfo>,
    pub log: CommitLog,
    pub depth: usize,
    pub ops_budget: usize,
}

fn invalid(msg: &'static str) -> Status {
    Status::invalid_argument(msg)
}

fn utf8(bytes: &[u8], what: &'static str) -> Result<String, Status> {
    String::from_utf8(bytes.to_vec()).map_err(|e| {
        Status::invalid_argument(format!("{} is not valid utf-8: {}", what, e))
    })
}

/// etcd range selection shared by range/delete/compare:
/// empty range_end -> single key, "\0" -> all keys >= key,
/// otherwise [key, range_end) with an exclusive upper bound.
fn select_keys(tree: &TrieMap, key: &str, range_end: &str) -> Vec<KVPair> {
    if range_end.is_empty() {
        tree.get(key).into_iter().collect()
    } else if range_end == "\0" {
        tree.scan(key, None)
    } else {
        tree.scan(key, Some(range_end))
    }
}

// ---------------------------------------------------------------------------
// Operation cores
// ---------------------------------------------------------------------------

pub(crate) fn range_core(tree: &TrieMap, req: &RangeRequest) -> Result<RangeResponse, Status> {
    let key = utf8(&req.key, "key")?;
    let range_end = utf8(&req.range_end, "range_end")?;

    if req.keys_only && req.count_only {
        return Err(invalid("keys_only and count_only cannot be set together"));
    }

    let matches = select_keys(tree, &key, &range_end);
    let count = matches.len() as i64;
    let limit = if req.limit > 0 {
        Some(req.limit as usize)
    } else {
        None
    };
    let more = limit.is_some_and(|l| count > l as i64);

    let mut kvs = Vec::new();
    for item in matches.into_iter().take(limit.unwrap_or(usize::MAX)) {
        if req.count_only {
            continue;
        }
        let value = if req.keys_only { "" } else { item.1.as_str() };
        kvs.push(create_kv(item.0.as_str(), value));
    }

    Ok(RangeResponse { kvs, more, count })
}

pub(crate) fn put_core(
    tree: &mut TrieMap,
    leases: &HashMap<i64, LeaseInfo>,
    req: &PutRequest,
    log: &mut CommitLog,
) -> Result<PutResponse, Status> {
    if req.key.is_empty() {
        return Err(invalid("key is not provided"));
    }
    let key = utf8(&req.key, "key")?;
    let val = utf8(&req.value, "value")?;

    if req.lease != 0 && !leases.contains_key(&req.lease) {
        return Err(Status::failed_precondition(format!(
            "lease {} not found",
            req.lease
        )));
    }

    let old = tree.put(&key, &val);
    log.mutated = true;
    if req.lease != 0 {
        log.lease_attach.push((req.lease, key.clone()));
    }

    let prev_kv = old.map(|kv| create_kv(&kv.0, &kv.1));
    log.events.push(Event {
        r#type: 0, // PUT
        kv: Some(create_kv(&key, &val)),
        prev_kv: if req.prev_kv { prev_kv.clone() } else { None },
    });

    Ok(PutResponse {
        prev_kv: if req.prev_kv { prev_kv } else { None },
    })
}

pub(crate) fn delete_core(
    tree: &mut TrieMap,
    req: &DeleteRangeRequest,
    log: &mut CommitLog,
) -> Result<DeleteRangeResponse, Status> {
    let key = utf8(&req.key, "key")?;
    let range_end = utf8(&req.range_end, "range_end")?;

    let matches = select_keys(tree, &key, &range_end);
    let count = matches.len() as i64;

    let mut prev_kvs = Vec::new();
    for item in matches {
        tree.delete(item.0.as_str());
        log.mutated = true;
        log.detached_keys.push(item.0.clone());
        prev_kvs.push(create_kv(item.0.as_str(), item.1.as_str()));
    }

    // a delete event carries the deleted key itself (etcd semantics)
    for kv in &prev_kvs {
        let key = String::from_utf8_lossy(&kv.key);
        log.events.push(Event {
            r#type: 1, // DELETE
            kv: Some(create_kv(&key, "")),
            prev_kv: Some(kv.clone()),
        });
    }

    Ok(DeleteRangeResponse {
        deleted: count,
        prev_kvs: if req.prev_kv { prev_kvs } else { vec![] },
    })
}

/// lease id a key is attached to, found by reverse lookup; 0 for keys
/// without a lease (etcd zero value).
fn key_lease(leases: &HashMap<i64, LeaseInfo>, key: &str) -> i64 {
    for (id, info) in leases {
        if info.keys.contains(key) {
            return *id;
        }
    }
    0
}

fn compare_numeric(actual: i64, wanted: i64, result: CompareResult) -> bool {
    match result {
        CompareResult::Equal => actual == wanted,
        CompareResult::Greater => actual > wanted,
        CompareResult::Less => actual < wanted,
        CompareResult::NotEqual => actual != wanted,
    }
}

/// Evaluate one Compare against the (shadow) tree.
fn eval_compare(ctx: &TxnCtx<'_>, cmp: &Compare) -> Result<bool, Status> {
    if cmp.key.is_empty() {
        return Err(invalid("key is not provided"));
    }
    let result = CompareResult::try_from(cmp.result)
        .map_err(|_| invalid("invalid compare result"))?;
    let target = CompareTarget::try_from(cmp.target)
        .map_err(|_| invalid("invalid compare target"))?;
    let wanted = cmp
        .target_union
        .as_ref()
        .ok_or_else(|| invalid("compare target value is not set"))?;

    let key = utf8(&cmp.key, "key")?;
    let range_end = utf8(&cmp.range_end, "range_end")?;
    let matches = select_keys(ctx.tree, &key, &range_end);

    // VALUE on a missing key is always false, like etcd: "no value" is not
    // distinguishable from an empty value over gRPC. Numeric targets on a
    // missing key compare against the etcd zero value (0).
    if matches.is_empty() {
        return match wanted {
            compare::TargetUnion::Value(_) => Ok(false),
            compare::TargetUnion::Version(w) => Ok(compare_numeric(0, *w, result)),
            compare::TargetUnion::CreateRevision(w) => Ok(compare_numeric(0, *w, result)),
            compare::TargetUnion::ModRevision(w) => Ok(compare_numeric(0, *w, result)),
            compare::TargetUnion::Lease(w) => Ok(compare_numeric(0, *w, result)),
        };
    }

    for kv in &matches {
        let ok = match (target, wanted) {
            (CompareTarget::Value, compare::TargetUnion::Value(w)) => {
                compare_bytes(kv.1.as_bytes(), w, result)
            }
            (CompareTarget::Version, compare::TargetUnion::Version(w)) => {
                compare_numeric(1, *w, result)
            }
            (CompareTarget::Create, compare::TargetUnion::CreateRevision(w)) => {
                compare_numeric(1, *w, result)
            }
            (CompareTarget::Mod, compare::TargetUnion::ModRevision(w)) => {
                compare_numeric(1, *w, result)
            }
            (CompareTarget::Lease, compare::TargetUnion::Lease(w)) => {
                compare_numeric(key_lease(ctx.leases, &kv.0), *w, result)
            }
            _ => return Err(invalid("compare target value type mismatch")),
        };
        // a range compare holds only if EVERY matched key satisfies it
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_bytes(actual: &[u8], wanted: &[u8], result: CompareResult) -> bool {
    match result {
        CompareResult::Equal => actual == wanted,
        CompareResult::Greater => actual > wanted,
        CompareResult::Less => actual < wanted,
        CompareResult::NotEqual => actual != wanted,
    }
}

// ---------------------------------------------------------------------------
// Txn application
// ---------------------------------------------------------------------------

pub(crate) fn apply_txn(ctx: &mut TxnCtx<'_>, req: &TxnRequest) -> Result<TxnResponse, Status> {
    if ctx.depth + 1 > MAX_TXN_DEPTH {
        return Err(invalid("txn nesting too deep"));
    }
    if req.compare.len() > MAX_TXN_OPS
        || req.success.len() > MAX_TXN_OPS
        || req.failure.len() > MAX_TXN_OPS
    {
        return Err(invalid("too many operations in txn request"));
    }

    let succeeded = eval_guard(ctx, &req.compare)?;

    let ops = if succeeded { &req.success } else { &req.failure };
    check_write_conflicts(ops)?;

    if ctx.ops_budget < ops.len() {
        return Err(invalid("txn operation budget exceeded"));
    }
    ctx.ops_budget -= ops.len();
    ctx.depth += 1;
    let mut responses = Vec::with_capacity(ops.len());
    for op in ops {
        let response = match &op.request {
            Some(request_op::Request::RequestRange(r)) => {
                ResponseOp {
                    response: Some(response_op::Response::ResponseRange(range_core(
                        ctx.tree, r,
                    )?)),
                }
            }
            Some(request_op::Request::RequestPut(p)) => ResponseOp {
                response: Some(response_op::Response::ResponsePut(put_core(
                    ctx.tree, ctx.leases, p, &mut ctx.log,
                )?)),
            },
            Some(request_op::Request::RequestDeleteRange(d)) => ResponseOp {
                response: Some(response_op::Response::ResponseDeleteRange(delete_core(
                    ctx.tree, d, &mut ctx.log,
                )?)),
            },
            Some(request_op::Request::RequestTxn(t)) => {
                let inner = apply_txn(ctx, t)?;
                ResponseOp {
                    response: Some(response_op::Response::ResponseTxn(inner)),
                }
            }
            None => return Err(invalid("empty request op")),
        };
        responses.push(response);
    }
    ctx.depth -= 1;

    Ok(TxnResponse {
        succeeded,
        responses,
    })
}

fn eval_guard(ctx: &TxnCtx<'_>, guard: &[Compare]) -> Result<bool, Status> {
    for cmp in guard {
        if !eval_compare(ctx, cmp)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// etcd checkIntervals: a txn may not put the same key twice, nor put a key
/// that its own delete_range removes. Overlapping delete ranges are fine.
fn check_write_conflicts(ops: &[RequestOp]) -> Result<(), Status> {
    let mut puts: HashSet<String> = HashSet::new();
    let mut delete_intervals: Vec<(String, String)> = Vec::new(); // (key, range_end)

    for op in ops {
        match &op.request {
            Some(request_op::Request::RequestPut(p)) => {
                let key = utf8(&p.key, "key")?;
                if !puts.insert(key.clone()) {
                    return Err(invalid("duplicate key given in txn request"));
                }
            }
            Some(request_op::Request::RequestDeleteRange(d)) => {
                delete_intervals.push((utf8(&d.key, "key")?, utf8(&d.range_end, "range_end")?));
            }
            _ => {}
        }
    }

    for key in &puts {
        let key = key.as_str();
        for (dkey, dend) in &delete_intervals {
            let (dkey, dend) = (dkey.as_str(), dend.as_str());
            let inside = if dend.is_empty() {
                key == dkey
            } else if dend == "\0" {
                key >= dkey
            } else {
                key >= dkey && key < dend
            };
            if inside {
                return Err(invalid("duplicate key given in txn request"));
            }
        }
    }
    Ok(())
}
