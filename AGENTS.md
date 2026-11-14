# AGENTS.md

Guidance for coding agents (and humans) working in this repository.

## Project

`galadh` is a simple key-value storage inspired by `etcd`, written in Rust.
It aims to be wire-compatible with etcd's gRPC API (`etcdctl` compatibility is the goal).
Project status: early alpha, educational. Data is in-memory only — there is no persistence yet.

## Layout

- `proto/galadh.proto` — the API definition (simplified copy of etcd's KV/Watch/Lease services)
- `src/proto/` — **generated** by `build.rs` from the `.proto`. Never hand-edit.
- `src/server.rs` — `gldh` server binary (tonic). Currently hardcodes `127.0.0.1:2379`.
- `src/client.rs` — `gldh-cli` client binary (clap + tonic).
- `src/stash/` — the `Stash` service implementing KV/Lease/Watch traits; range/delete logic lives here.
- `src/trie/` — own prefix tree (`TrieMap`), `String`-keyed, sorted traversal.

## Build & test

`protoc` is required (`apt-get install protobuf-compiler`, or set `PROTOC`).

```bash
cargo build          # builds gldh and gldh-cli
cargo test           # unit tests (stash + trie), no gRPC needed
cargo clippy --all-targets
```

Smoke test by hand:

```bash
./target/debug/gldh &                      # server
./target/debug/gldh-cli put key value      # client: put | get [--prefix] [--limit N] | delete [--prefix]
```

## Invariants — do not break these

1. **etcd range semantics** in `Stash::range` / `Stash::delete_range`:
   - empty `range_end` → exact key lookup only
   - `range_end == "\0"` → all keys `>= key`
   - otherwise → `[key, range_end)` with an **exclusive** upper bound
   - prefix scan = `range_end = key+1` (successor with 0xff carry, see `prefix_end` in `src/stash/utils.rs` and `src/client.rs`)
   - `limit` must be applied; `count` = number of keys in range (not capped by limit); `more` = `count > returned`
2. **Never `todo!()` in rpc handlers** — a panic kills the connection. Endpoints that are
   not implemented return `Status::unimplemented(...)` (currently: `Txn`, all `Lease` rpcs, `Watch`).
3. **Bad client input → `Status::invalid_argument`**, not `unknown`.
4. `keys_only` and `count_only` together are rejected.

## Known limitations (open work, don't "fix" silently)

- No persistence: restart wipes data.
- Range scans traverse the whole trie and filter afterwards — O(all keys).
- Keys/values are `String`s: non-UTF-8 (binary) keys are rejected, unlike etcd.
- No CI, no branch protection. `main` should stay green: run `cargo test` before pushing.
- Server address is hardcoded; no config flags yet.

## Conventions

- English for code, comments, commit messages and docs.
- Commit style: short imperative summary, optional body with bullet points.
- Tests live inline in `#[cfg(test)] mod tests` next to the code they cover.
- Prefer extending existing tests over adding a new test harness; keep `cargo clippy` warnings in handwritten code out of new commits (generated `src/proto/` is exempt).
