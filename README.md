# galadh key-value storage
[Github repo](https://github.com/thekitaev/galadh)

`galadh` ([sindar](https://glosbe.com/sjn/en/galadh) word for tree) is a key-value storage inspired by `etcd`.

It was initially supposed to operate like single-instance `etcd` and be fully compatible with `etcdctl` utility.

It's based on simplified etcd's grpc-protobuffs (further rework needed).
Project goal is educational. Current state is early alpha.

It comes with two binaries: `gldh` is a server and `gldh-cli` is a client

### How-to
```bash
# run server (add --data-dir DIR to persist across restarts, --addr HOST:PORT to pick an address)
$ gldh
# use client in another terminal
$ gldh-cli put key value
$ gldh-cli get key
key
value
```

### TODO (server)
- [x] get/put 
- [x] delete
- [x] snapshots and restoring from snapshot (`--data-dir`)
- [x] running params (`--addr`, `--data-dir`)
- [x] replace prefix tree dep or implement an own one
- [x] watchers (create/cancel, prefix/exact/range, filters, prev_kv)
- [x] leases (grant/revoke/keepalive/time-to-live/expiry; lease attachments are not persisted)
- [ ] transactions
- [x] tests
- [ ] docs
- [ ] cluster
- [ ] ???

### TODO (client)
- [x] get
- [x] put
- [x] connection params (`--addr`)
- [x] delete
- [ ] watch
- [ ] transactions
- [ ] ???
