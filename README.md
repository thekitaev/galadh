# galadh key-value storage
[Github repo](https://github.com/thekitaev/galadh)

`galadh` ([sindar](https://glosbe.com/sjn/en/galadh) word for tree) is a key-value storage inspired by `etcd`.

It was initially supposed to operate like single-instance `etcd` and be fully compatible with `etcdctl` utility.

It's based on simplified etcd's grpc-protobuffs (further rework needed).
Project goal is educational. Current state is early alpha.

It comes with two binaries: `gldh` is a server and `gldh-cli` is a client

### How-to
```bash
# run server
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
- [ ] snapshots and restoring from snapshot
- [ ] running params
- [ ] replace prefix tree dep or implement an own one
- [ ] watchers
- [ ] leases
- [ ] transactions
- [ ] tests
- [ ] docs
- [ ] cluster
- [ ] ???

### TODO (client)
- [x] get (partially)
- [ ] put
- [ ] connection params
- [ ] delete
- [ ] watch
- [ ] transactions
- [ ] ???
