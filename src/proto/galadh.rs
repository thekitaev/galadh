#[derive(Clone, PartialEq, ::prost::Message)]
pub struct KeyValue {
    /// key is the key in bytes. An empty key is not allowed.
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// value is the value held by the key, in bytes.
    #[prost(bytes = "vec", tag = "2")]
    pub value: ::prost::alloc::vec::Vec<u8>,
    /// lease is the ID of the lease that attached to key.
    /// When the attached lease expires, the key will be deleted.
    /// If lease is 0, then no lease is attached to the key.
    #[prost(int64, tag = "3")]
    pub lease: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Event {
    /// type is the kind of event. If type is a PUT, it indicates
    /// new data has been stored to the key. If type is a DELETE,
    /// it indicates the key was deleted.
    #[prost(enumeration = "event::EventType", tag = "1")]
    pub r#type: i32,
    /// kv holds the KeyValue for the event.
    /// A PUT event contains current kv pair.
    /// A PUT event with kv.Version=1 indicates the creation of a key.
    /// A DELETE/EXPIRE event contains the deleted key with
    /// its modification revision set to the revision of deletion.
    #[prost(message, optional, tag = "2")]
    pub kv: ::core::option::Option<KeyValue>,
    /// prev_kv holds the key-value pair before the event happens.
    #[prost(message, optional, tag = "3")]
    pub prev_kv: ::core::option::Option<KeyValue>,
}
/// Nested message and enum types in `Event`.
pub mod event {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum EventType {
        Put = 0,
        Delete = 1,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RangeRequest {
    /// key is the first key for the range. If range_end is not given, the request only looks up key.
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// range_end is the upper bound on the requested range [key, range_end).
    /// If range_end is '\0', the range is all keys >= key.
    /// If range_end is key plus one (e.g., "aa"+1 == "ab", "a\xff"+1 == "b"),
    /// then the range request gets all keys prefixed with key.
    /// If both key and range_end are '\0', then the range request returns all keys.
    #[prost(bytes = "vec", tag = "2")]
    pub range_end: ::prost::alloc::vec::Vec<u8>,
    /// limit is a limit on the number of keys returned for the request. When limit is set to 0,
    /// it is treated as no limit.
    #[prost(int64, tag = "3")]
    pub limit: i64,
    /// sort_order is the order for returned sorted results.
    #[prost(enumeration = "range_request::SortOrder", tag = "4")]
    pub sort_order: i32,
    /// sort_target is the key-value field to use for sorting.
    #[prost(enumeration = "range_request::SortTarget", tag = "5")]
    pub sort_target: i32,
    /// keys_only when set returns only the keys and not the values.
    #[prost(bool, tag = "6")]
    pub keys_only: bool,
    /// count_only when set returns only the count of the keys in the range.
    #[prost(bool, tag = "7")]
    pub count_only: bool,
}
/// Nested message and enum types in `RangeRequest`.
pub mod range_request {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum SortOrder {
        /// default, no sorting
        None = 0,
        /// lowest target value first
        Ascend = 1,
        /// highest target value first
        Descend = 2,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum SortTarget {
        Key = 0,
        Version = 1,
        Create = 2,
        Mod = 3,
        Value = 4,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RangeResponse {
    /// kvs is the list of key-value pairs matched by the range request.
    /// kvs is empty when count is requested.
    #[prost(message, repeated, tag = "1")]
    pub kvs: ::prost::alloc::vec::Vec<KeyValue>,
    /// more indicates if there are more keys to return in the requested range.
    #[prost(bool, tag = "2")]
    pub more: bool,
    /// count is set to the number of keys within the range when requested.
    #[prost(int64, tag = "3")]
    pub count: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PutRequest {
    /// key is the key, in bytes, to put into the key-value store.
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// value is the value, in bytes, to associate with the key in the key-value store.
    #[prost(bytes = "vec", tag = "2")]
    pub value: ::prost::alloc::vec::Vec<u8>,
    /// lease is the lease ID to associate with the key in the key-value store. A lease
    /// value of 0 indicates no lease.
    #[prost(int64, tag = "3")]
    pub lease: i64,
    /// If prev_kv is set, etcd gets the previous key-value pair before changing it.
    /// The previous key-value pair will be returned in the put response.
    #[prost(bool, tag = "4")]
    pub prev_kv: bool,
    /// If ignore_value is set, etcd updates the key using its current value.
    /// Returns an error if the key does not exist.
    #[prost(bool, tag = "5")]
    pub ignore_value: bool,
    /// If ignore_lease is set, etcd updates the key using its current lease.
    /// Returns an error if the key does not exist.
    #[prost(bool, tag = "6")]
    pub ignore_lease: bool,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PutResponse {
    /// if prev_kv is set in the request, the previous key-value pair will be returned.
    #[prost(message, optional, tag = "1")]
    pub prev_kv: ::core::option::Option<KeyValue>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeleteRangeRequest {
    /// key is the first key to delete in the range.
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// range_end is the key following the last key to delete for the range [key, range_end).
    /// If range_end is not given, the range is defined to contain only the key argument.
    /// If range_end is one bit larger than the given key, then the range is all the keys
    /// with the prefix (the given key).
    /// If range_end is '\0', the range is all keys greater than or equal to the key argument.
    #[prost(bytes = "vec", tag = "2")]
    pub range_end: ::prost::alloc::vec::Vec<u8>,
    /// If prev_kv is set, etcd gets the previous key-value pairs before deleting it.
    /// The previous key-value pairs will be returned in the delete response.
    #[prost(bool, tag = "3")]
    pub prev_kv: bool,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeleteRangeResponse {
    /// deleted is the number of keys deleted by the delete range request.
    #[prost(int64, tag = "1")]
    pub deleted: i64,
    /// if prev_kv is set in the request, the previous key-value pairs will be returned.
    #[prost(message, repeated, tag = "2")]
    pub prev_kvs: ::prost::alloc::vec::Vec<KeyValue>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RequestOp {
    /// request is a union of request types accepted by a transaction.
    #[prost(oneof = "request_op::Request", tags = "1, 2, 3, 4")]
    pub request: ::core::option::Option<request_op::Request>,
}
/// Nested message and enum types in `RequestOp`.
pub mod request_op {
    /// request is a union of request types accepted by a transaction.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Request {
        #[prost(message, tag = "1")]
        RequestRange(super::RangeRequest),
        #[prost(message, tag = "2")]
        RequestPut(super::PutRequest),
        #[prost(message, tag = "3")]
        RequestDeleteRange(super::DeleteRangeRequest),
        #[prost(message, tag = "4")]
        RequestTxn(super::TxnRequest),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ResponseOp {
    /// response is a union of response types returned by a transaction.
    #[prost(oneof = "response_op::Response", tags = "1, 2, 3, 4")]
    pub response: ::core::option::Option<response_op::Response>,
}
/// Nested message and enum types in `ResponseOp`.
pub mod response_op {
    /// response is a union of response types returned by a transaction.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Response {
        #[prost(message, tag = "1")]
        ResponseRange(super::RangeResponse),
        #[prost(message, tag = "2")]
        ResponsePut(super::PutResponse),
        #[prost(message, tag = "3")]
        ResponseDeleteRange(super::DeleteRangeResponse),
        #[prost(message, tag = "4")]
        ResponseTxn(super::TxnResponse),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Compare {
    /// result is logical comparison operation for this comparison.
    #[prost(enumeration = "compare::CompareResult", tag = "1")]
    pub result: i32,
    /// target is the key-value field to inspect for the comparison.
    #[prost(enumeration = "compare::CompareTarget", tag = "2")]
    pub target: i32,
    /// key is the subject key for the comparison operation.
    #[prost(bytes = "vec", tag = "3")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// range_end compares the given target to all keys in the range [key, range_end).
    /// See RangeRequest for more details on key ranges.
    ///
    /// TODO: fill out with most of the rest of RangeRequest fields when needed.
    #[prost(bytes = "vec", tag = "64")]
    pub range_end: ::prost::alloc::vec::Vec<u8>,
    #[prost(oneof = "compare::TargetUnion", tags = "4, 5, 6, 7, 8")]
    pub target_union: ::core::option::Option<compare::TargetUnion>,
}
/// Nested message and enum types in `Compare`.
pub mod compare {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum CompareResult {
        Equal = 0,
        Greater = 1,
        Less = 2,
        NotEqual = 3,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum CompareTarget {
        Version = 0,
        Create = 1,
        Mod = 2,
        Value = 3,
        Lease = 4,
    }
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum TargetUnion {
        /// version is the version of the given key
        #[prost(int64, tag = "4")]
        Version(i64),
        /// create_revision is the creation revision of the given key
        #[prost(int64, tag = "5")]
        CreateRevision(i64),
        /// mod_revision is the last modified revision of the given key.
        #[prost(int64, tag = "6")]
        ModRevision(i64),
        /// value is the value of the given key, in bytes.
        #[prost(bytes, tag = "7")]
        Value(::prost::alloc::vec::Vec<u8>),
        /// lease is the lease id of the given key.
        ///
        /// leave room for more target_union field tags, jump to 64
        #[prost(int64, tag = "8")]
        Lease(i64),
    }
}
/// From google paxosdb paper:
/// Our implementation hinges around a powerful primitive which we call MultiOp. All other database
/// operations except for iteration are implemented as a single call to MultiOp. A MultiOp is applied atomically
/// and consists of three components:
/// 1. A list of tests called guard. Each test in guard checks a single entry in the database. It may check
/// for the absence or presence of a value, or compare with a given value. Two different tests in the guard
/// may apply to the same or different entries in the database. All tests in the guard are applied and
/// MultiOp returns the results. If all tests are true, MultiOp executes t op (see item 2 below), otherwise
/// it executes f op (see item 3 below).
/// 2. A list of database operations called t op. Each operation in the list is either an insert, delete, or
/// lookup operation, and applies to a single database entry. Two different operations in the list may apply
/// to the same or different entries in the database. These operations are executed
/// if guard evaluates to
/// true.
/// 3. A list of database operations called f op. Like t op, but executed if guard evaluates to false.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TxnRequest {
    /// compare is a list of predicates representing a conjunction of terms.
    /// If the comparisons succeed, then the success requests will be processed in order,
    /// and the response will contain their respective responses in order.
    /// If the comparisons fail, then the failure requests will be processed in order,
    /// and the response will contain their respective responses in order.
    #[prost(message, repeated, tag = "1")]
    pub compare: ::prost::alloc::vec::Vec<Compare>,
    /// success is a list of requests which will be applied when compare evaluates to true.
    #[prost(message, repeated, tag = "2")]
    pub success: ::prost::alloc::vec::Vec<RequestOp>,
    /// failure is a list of requests which will be applied when compare evaluates to false.
    #[prost(message, repeated, tag = "3")]
    pub failure: ::prost::alloc::vec::Vec<RequestOp>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TxnResponse {
    /// succeeded is set to true if the compare evaluated to true or false otherwise.
    #[prost(bool, tag = "1")]
    pub succeeded: bool,
    /// responses is a list of responses corresponding to the results from applying
    /// success if succeeded is true or failure if succeeded is false.
    #[prost(message, repeated, tag = "2")]
    pub responses: ::prost::alloc::vec::Vec<ResponseOp>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct WatchRequest {
    /// request_union is a request to either create a new watcher or cancel an existing watcher.
    #[prost(oneof = "watch_request::RequestUnion", tags = "1, 2, 3")]
    pub request_union: ::core::option::Option<watch_request::RequestUnion>,
}
/// Nested message and enum types in `WatchRequest`.
pub mod watch_request {
    /// request_union is a request to either create a new watcher or cancel an existing watcher.
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum RequestUnion {
        #[prost(message, tag = "1")]
        CreateRequest(super::WatchCreateRequest),
        #[prost(message, tag = "2")]
        CancelRequest(super::WatchCancelRequest),
        #[prost(message, tag = "3")]
        ProgressRequest(super::WatchProgressRequest),
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct WatchCreateRequest {
    /// key is the key to register for watching.
    #[prost(bytes = "vec", tag = "1")]
    pub key: ::prost::alloc::vec::Vec<u8>,
    /// range_end is the end of the range [key, range_end) to watch. If range_end is not given,
    /// only the key argument is watched. If range_end is equal to '\0', all keys greater than
    /// or equal to the key argument are watched.
    /// If the range_end is one bit larger than the given key,
    /// then all keys with the prefix (the given key) will be watched.
    #[prost(bytes = "vec", tag = "2")]
    pub range_end: ::prost::alloc::vec::Vec<u8>,
    /// filters filter the events at server side before it sends back to the watcher.
    #[prost(enumeration = "watch_create_request::FilterType", repeated, tag = "3")]
    pub filters: ::prost::alloc::vec::Vec<i32>,
    /// If prev_kv is set, created watcher gets the previous KV before the event happens.
    /// If the previous KV is already compacted, nothing will be returned.
    #[prost(bool, tag = "4")]
    pub prev_kv: bool,
    /// If watch_id is provided and non-zero, it will be assigned to this watcher.
    /// Since creating a watcher in etcd is not a synchronous operation,
    /// this can be used ensure that ordering is correct when creating multiple
    /// watchers on the same stream. Creating a watcher with an ID already in
    /// use on the stream will cause an error to be returned.
    #[prost(int64, tag = "5")]
    pub watch_id: i64,
}
/// Nested message and enum types in `WatchCreateRequest`.
pub mod watch_create_request {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum FilterType {
        /// filter out put event.
        Noput = 0,
        /// filter out delete event.
        Nodelete = 1,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct WatchCancelRequest {
    /// watch_id is the watcher id to cancel so that no more events are transmitted.
    #[prost(int64, tag = "1")]
    pub watch_id: i64,
}
/// Requests the a watch stream progress status be sent in the watch response stream as soon as
/// possible.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct WatchProgressRequest {}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct WatchResponse {
    /// watch_id is the ID of the watcher that corresponds to the response.
    #[prost(int64, tag = "1")]
    pub watch_id: i64,
    /// created is set to true if the response is for a create watch request.
    /// The client should record the watch_id and expect to receive events for
    /// the created watcher from the same stream.
    /// All events sent to the created watcher will attach with the same watch_id.
    #[prost(bool, tag = "2")]
    pub created: bool,
    /// canceled is set to true if the response is for a cancel watch request.
    /// No further events will be sent to the canceled watcher.
    #[prost(bool, tag = "3")]
    pub canceled: bool,
    /// compact_revision is set to the minimum index if a watcher tries to watch
    /// at a compacted index.
    ///
    /// This happens when creating a watcher at a compacted revision or the watcher cannot
    /// catch up with the progress of the key-value store.
    ///
    /// The client should treat the watcher as canceled and should not try to create any
    /// watcher with the same start_revision again.
    #[prost(int64, tag = "4")]
    pub compact_revision: i64,
    /// cancel_reason indicates the reason for canceling the watcher.
    #[prost(string, tag = "5")]
    pub cancel_reason: ::prost::alloc::string::String,
    /// framgment is true if large watch response was split over multiple responses.
    #[prost(bool, tag = "6")]
    pub fragment: bool,
    #[prost(message, repeated, tag = "7")]
    pub events: ::prost::alloc::vec::Vec<Event>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseGrantRequest {
    /// TTL is the advisory time-to-live in seconds. Expired lease will return -1.
    #[prost(int64, tag = "1")]
    pub ttl: i64,
    /// ID is the requested ID for the lease. If ID is set to 0, the lessor chooses an ID.
    #[prost(int64, tag = "2")]
    pub id: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseGrantResponse {
    /// ID is the lease ID for the granted lease.
    #[prost(int64, tag = "1")]
    pub id: i64,
    /// TTL is the server chosen lease time-to-live in seconds.
    #[prost(int64, tag = "2")]
    pub ttl: i64,
    #[prost(string, tag = "3")]
    pub error: ::prost::alloc::string::String,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseRevokeRequest {
    /// ID is the lease ID to revoke. When the ID is revoked, all associated keys will be deleted.
    #[prost(int64, tag = "1")]
    pub id: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseRevokeResponse {}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseKeepAliveRequest {
    /// ID is the lease ID for the lease to keep alive.
    #[prost(int64, tag = "1")]
    pub id: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseKeepAliveResponse {
    /// ID is the lease ID from the keep alive request.
    #[prost(int64, tag = "1")]
    pub id: i64,
    /// TTL is the new time-to-live for the lease.
    #[prost(int64, tag = "2")]
    pub ttl: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseTimeToLiveRequest {
    /// ID is the lease ID for the lease.
    #[prost(int64, tag = "1")]
    pub id: i64,
    /// keys is true to query all the keys attached to this lease.
    #[prost(bool, tag = "2")]
    pub keys: bool,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseTimeToLiveResponse {
    /// ID is the lease ID from the keep alive request.
    #[prost(int64, tag = "1")]
    pub id: i64,
    /// TTL is the remaining TTL in seconds for the lease; the lease will expire in under TTL+1 seconds.
    #[prost(int64, tag = "2")]
    pub ttl: i64,
    /// GrantedTTL is the initial granted time in seconds upon lease creation/renewal.
    #[prost(int64, tag = "3")]
    pub granted_ttl: i64,
    /// Keys is the list of keys attached to this lease.
    #[prost(bytes = "vec", repeated, tag = "4")]
    pub keys: ::prost::alloc::vec::Vec<::prost::alloc::vec::Vec<u8>>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseLeasesRequest {}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseStatus {
    /// TODO: int64 TTL = 2;
    #[prost(int64, tag = "1")]
    pub id: i64,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LeaseLeasesResponse {
    #[prost(message, repeated, tag = "1")]
    pub leases: ::prost::alloc::vec::Vec<LeaseStatus>,
}
#[doc = r" Generated client implementations."]
pub mod kv_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[derive(Debug, Clone)]
    pub struct KvClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl KvClient<tonic::transport::Channel> {
        #[doc = r" Attempt to create a new client by connecting to a given endpoint."]
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: std::convert::TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> KvClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::ResponseBody: Body + Send + 'static,
        T::Error: Into<StdError>,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> KvClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<http::Request<tonic::body::BoxBody>>>::Error:
                Into<StdError> + Send + Sync,
        {
            KvClient::new(InterceptedService::new(inner, interceptor))
        }
        #[doc = r" Compress requests with `gzip`."]
        #[doc = r""]
        #[doc = r" This requires the server to support it otherwise it might respond with an"]
        #[doc = r" error."]
        pub fn send_gzip(mut self) -> Self {
            self.inner = self.inner.send_gzip();
            self
        }
        #[doc = r" Enable decompressing responses with `gzip`."]
        pub fn accept_gzip(mut self) -> Self {
            self.inner = self.inner.accept_gzip();
            self
        }
        #[doc = " Range gets the keys in the range from the key-value store."]
        pub async fn range(
            &mut self,
            request: impl tonic::IntoRequest<super::RangeRequest>,
        ) -> Result<tonic::Response<super::RangeResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.KV/Range");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Put puts the given key into the key-value store."]
        #[doc = " A put request increments the revision of the key-value store"]
        #[doc = " and generates one event in the event history."]
        pub async fn put(
            &mut self,
            request: impl tonic::IntoRequest<super::PutRequest>,
        ) -> Result<tonic::Response<super::PutResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.KV/Put");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " DeleteRange deletes the given range from the key-value store."]
        #[doc = " A delete request increments the revision of the key-value store"]
        #[doc = " and generates a delete event in the event history for every deleted key."]
        pub async fn delete_range(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteRangeRequest>,
        ) -> Result<tonic::Response<super::DeleteRangeResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.KV/DeleteRange");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " Txn processes multiple requests in a single transaction."]
        #[doc = " A txn request increments the revision of the key-value store"]
        #[doc = " and generates events with the same revision for every completed request."]
        #[doc = " It is not allowed to modify the same key several times within one txn."]
        pub async fn txn(
            &mut self,
            request: impl tonic::IntoRequest<super::TxnRequest>,
        ) -> Result<tonic::Response<super::TxnResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.KV/Txn");
            self.inner.unary(request.into_request(), path, codec).await
        }
    }
}
#[doc = r" Generated client implementations."]
pub mod watch_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[derive(Debug, Clone)]
    pub struct WatchClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl WatchClient<tonic::transport::Channel> {
        #[doc = r" Attempt to create a new client by connecting to a given endpoint."]
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: std::convert::TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> WatchClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::ResponseBody: Body + Send + 'static,
        T::Error: Into<StdError>,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> WatchClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<http::Request<tonic::body::BoxBody>>>::Error:
                Into<StdError> + Send + Sync,
        {
            WatchClient::new(InterceptedService::new(inner, interceptor))
        }
        #[doc = r" Compress requests with `gzip`."]
        #[doc = r""]
        #[doc = r" This requires the server to support it otherwise it might respond with an"]
        #[doc = r" error."]
        pub fn send_gzip(mut self) -> Self {
            self.inner = self.inner.send_gzip();
            self
        }
        #[doc = r" Enable decompressing responses with `gzip`."]
        pub fn accept_gzip(mut self) -> Self {
            self.inner = self.inner.accept_gzip();
            self
        }
        #[doc = " Watch watches for events happening or that have happened. Both input and output"]
        #[doc = " are streams; the input stream is for creating and canceling watchers and the output"]
        #[doc = " stream sends events. One watch RPC can watch on multiple key ranges, streaming events"]
        #[doc = " for several watches at once. The entire event history can be watched starting from the"]
        #[doc = " last compaction revision."]
        pub async fn watch(
            &mut self,
            request: impl tonic::IntoStreamingRequest<Message = super::WatchRequest>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::WatchResponse>>, tonic::Status>
        {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.Watch/Watch");
            self.inner
                .streaming(request.into_streaming_request(), path, codec)
                .await
        }
    }
}
#[doc = r" Generated client implementations."]
pub mod lease_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[derive(Debug, Clone)]
    pub struct LeaseClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl LeaseClient<tonic::transport::Channel> {
        #[doc = r" Attempt to create a new client by connecting to a given endpoint."]
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: std::convert::TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> LeaseClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::ResponseBody: Body + Send + 'static,
        T::Error: Into<StdError>,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> LeaseClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<http::Request<tonic::body::BoxBody>>>::Error:
                Into<StdError> + Send + Sync,
        {
            LeaseClient::new(InterceptedService::new(inner, interceptor))
        }
        #[doc = r" Compress requests with `gzip`."]
        #[doc = r""]
        #[doc = r" This requires the server to support it otherwise it might respond with an"]
        #[doc = r" error."]
        pub fn send_gzip(mut self) -> Self {
            self.inner = self.inner.send_gzip();
            self
        }
        #[doc = r" Enable decompressing responses with `gzip`."]
        pub fn accept_gzip(mut self) -> Self {
            self.inner = self.inner.accept_gzip();
            self
        }
        #[doc = " LeaseGrant creates a lease which expires if the server does not receive a keepAlive"]
        #[doc = " within a given time to live period. All keys attached to the lease will be expired and"]
        #[doc = " deleted if the lease expires. Each expired key generates a delete event in the event history."]
        pub async fn lease_grant(
            &mut self,
            request: impl tonic::IntoRequest<super::LeaseGrantRequest>,
        ) -> Result<tonic::Response<super::LeaseGrantResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.Lease/LeaseGrant");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " LeaseRevoke revokes a lease. All keys attached to the lease will expire and be deleted."]
        pub async fn lease_revoke(
            &mut self,
            request: impl tonic::IntoRequest<super::LeaseRevokeRequest>,
        ) -> Result<tonic::Response<super::LeaseRevokeResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.Lease/LeaseRevoke");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " LeaseKeepAlive keeps the lease alive by streaming keep alive requests from the client"]
        #[doc = " to the server and streaming keep alive responses from the server to the client."]
        pub async fn lease_keep_alive(
            &mut self,
            request: impl tonic::IntoStreamingRequest<Message = super::LeaseKeepAliveRequest>,
        ) -> Result<
            tonic::Response<tonic::codec::Streaming<super::LeaseKeepAliveResponse>>,
            tonic::Status,
        > {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.Lease/LeaseKeepAlive");
            self.inner
                .streaming(request.into_streaming_request(), path, codec)
                .await
        }
        #[doc = " LeaseTimeToLive retrieves lease information."]
        pub async fn lease_time_to_live(
            &mut self,
            request: impl tonic::IntoRequest<super::LeaseTimeToLiveRequest>,
        ) -> Result<tonic::Response<super::LeaseTimeToLiveResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.Lease/LeaseTimeToLive");
            self.inner.unary(request.into_request(), path, codec).await
        }
        #[doc = " LeaseLeases lists all existing leases."]
        pub async fn lease_leases(
            &mut self,
            request: impl tonic::IntoRequest<super::LeaseLeasesRequest>,
        ) -> Result<tonic::Response<super::LeaseLeasesResponse>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(
                    tonic::Code::Unknown,
                    format!("Service was not ready: {}", e.into()),
                )
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/galadh.Lease/LeaseLeases");
            self.inner.unary(request.into_request(), path, codec).await
        }
    }
}
#[doc = r" Generated server implementations."]
pub mod kv_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[doc = "Generated trait containing gRPC methods that should be implemented for use with KvServer."]
    #[async_trait]
    pub trait Kv: Send + Sync + 'static {
        #[doc = " Range gets the keys in the range from the key-value store."]
        async fn range(
            &self,
            request: tonic::Request<super::RangeRequest>,
        ) -> Result<tonic::Response<super::RangeResponse>, tonic::Status>;
        #[doc = " Put puts the given key into the key-value store."]
        #[doc = " A put request increments the revision of the key-value store"]
        #[doc = " and generates one event in the event history."]
        async fn put(
            &self,
            request: tonic::Request<super::PutRequest>,
        ) -> Result<tonic::Response<super::PutResponse>, tonic::Status>;
        #[doc = " DeleteRange deletes the given range from the key-value store."]
        #[doc = " A delete request increments the revision of the key-value store"]
        #[doc = " and generates a delete event in the event history for every deleted key."]
        async fn delete_range(
            &self,
            request: tonic::Request<super::DeleteRangeRequest>,
        ) -> Result<tonic::Response<super::DeleteRangeResponse>, tonic::Status>;
        #[doc = " Txn processes multiple requests in a single transaction."]
        #[doc = " A txn request increments the revision of the key-value store"]
        #[doc = " and generates events with the same revision for every completed request."]
        #[doc = " It is not allowed to modify the same key several times within one txn."]
        async fn txn(
            &self,
            request: tonic::Request<super::TxnRequest>,
        ) -> Result<tonic::Response<super::TxnResponse>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct KvServer<T: Kv> {
        inner: _Inner<T>,
        accept_compression_encodings: (),
        send_compression_encodings: (),
    }
    struct _Inner<T>(Arc<T>);
    impl<T: Kv> KvServer<T> {
        pub fn new(inner: T) -> Self {
            let inner = Arc::new(inner);
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
            }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for KvServer<T>
    where
        T: Kv,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = Never;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/galadh.KV/Range" => {
                    #[allow(non_camel_case_types)]
                    struct RangeSvc<T: Kv>(pub Arc<T>);
                    impl<T: Kv> tonic::server::UnaryService<super::RangeRequest> for RangeSvc<T> {
                        type Response = super::RangeResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RangeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).range(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = RangeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.KV/Put" => {
                    #[allow(non_camel_case_types)]
                    struct PutSvc<T: Kv>(pub Arc<T>);
                    impl<T: Kv> tonic::server::UnaryService<super::PutRequest> for PutSvc<T> {
                        type Response = super::PutResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PutRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).put(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = PutSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.KV/DeleteRange" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteRangeSvc<T: Kv>(pub Arc<T>);
                    impl<T: Kv> tonic::server::UnaryService<super::DeleteRangeRequest> for DeleteRangeSvc<T> {
                        type Response = super::DeleteRangeResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteRangeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).delete_range(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = DeleteRangeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.KV/Txn" => {
                    #[allow(non_camel_case_types)]
                    struct TxnSvc<T: Kv>(pub Arc<T>);
                    impl<T: Kv> tonic::server::UnaryService<super::TxnRequest> for TxnSvc<T> {
                        type Response = super::TxnResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TxnRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).txn(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = TxnSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => Box::pin(async move {
                    Ok(http::Response::builder()
                        .status(200)
                        .header("grpc-status", "12")
                        .header("content-type", "application/grpc")
                        .body(empty_body())
                        .unwrap())
                }),
            }
        }
    }
    impl<T: Kv> Clone for KvServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
            }
        }
    }
    impl<T: Kv> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Kv> tonic::transport::NamedService for KvServer<T> {
        const NAME: &'static str = "galadh.KV";
    }
}
#[doc = r" Generated server implementations."]
pub mod watch_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[doc = "Generated trait containing gRPC methods that should be implemented for use with WatchServer."]
    #[async_trait]
    pub trait Watch: Send + Sync + 'static {
        #[doc = "Server streaming response type for the Watch method."]
        type WatchStream: futures_core::Stream<Item = Result<super::WatchResponse, tonic::Status>>
            + Send
            + 'static;
        #[doc = " Watch watches for events happening or that have happened. Both input and output"]
        #[doc = " are streams; the input stream is for creating and canceling watchers and the output"]
        #[doc = " stream sends events. One watch RPC can watch on multiple key ranges, streaming events"]
        #[doc = " for several watches at once. The entire event history can be watched starting from the"]
        #[doc = " last compaction revision."]
        async fn watch(
            &self,
            request: tonic::Request<tonic::Streaming<super::WatchRequest>>,
        ) -> Result<tonic::Response<Self::WatchStream>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct WatchServer<T: Watch> {
        inner: _Inner<T>,
        accept_compression_encodings: (),
        send_compression_encodings: (),
    }
    struct _Inner<T>(Arc<T>);
    impl<T: Watch> WatchServer<T> {
        pub fn new(inner: T) -> Self {
            let inner = Arc::new(inner);
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
            }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for WatchServer<T>
    where
        T: Watch,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = Never;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/galadh.Watch/Watch" => {
                    #[allow(non_camel_case_types)]
                    struct WatchSvc<T: Watch>(pub Arc<T>);
                    impl<T: Watch> tonic::server::StreamingService<super::WatchRequest> for WatchSvc<T> {
                        type Response = super::WatchResponse;
                        type ResponseStream = T::WatchStream;
                        type Future =
                            BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<tonic::Streaming<super::WatchRequest>>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).watch(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = WatchSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => Box::pin(async move {
                    Ok(http::Response::builder()
                        .status(200)
                        .header("grpc-status", "12")
                        .header("content-type", "application/grpc")
                        .body(empty_body())
                        .unwrap())
                }),
            }
        }
    }
    impl<T: Watch> Clone for WatchServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
            }
        }
    }
    impl<T: Watch> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Watch> tonic::transport::NamedService for WatchServer<T> {
        const NAME: &'static str = "galadh.Watch";
    }
}
#[doc = r" Generated server implementations."]
pub mod lease_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    #[doc = "Generated trait containing gRPC methods that should be implemented for use with LeaseServer."]
    #[async_trait]
    pub trait Lease: Send + Sync + 'static {
        #[doc = " LeaseGrant creates a lease which expires if the server does not receive a keepAlive"]
        #[doc = " within a given time to live period. All keys attached to the lease will be expired and"]
        #[doc = " deleted if the lease expires. Each expired key generates a delete event in the event history."]
        async fn lease_grant(
            &self,
            request: tonic::Request<super::LeaseGrantRequest>,
        ) -> Result<tonic::Response<super::LeaseGrantResponse>, tonic::Status>;
        #[doc = " LeaseRevoke revokes a lease. All keys attached to the lease will expire and be deleted."]
        async fn lease_revoke(
            &self,
            request: tonic::Request<super::LeaseRevokeRequest>,
        ) -> Result<tonic::Response<super::LeaseRevokeResponse>, tonic::Status>;
        #[doc = "Server streaming response type for the LeaseKeepAlive method."]
        type LeaseKeepAliveStream: futures_core::Stream<Item = Result<super::LeaseKeepAliveResponse, tonic::Status>>
            + Send
            + 'static;
        #[doc = " LeaseKeepAlive keeps the lease alive by streaming keep alive requests from the client"]
        #[doc = " to the server and streaming keep alive responses from the server to the client."]
        async fn lease_keep_alive(
            &self,
            request: tonic::Request<tonic::Streaming<super::LeaseKeepAliveRequest>>,
        ) -> Result<tonic::Response<Self::LeaseKeepAliveStream>, tonic::Status>;
        #[doc = " LeaseTimeToLive retrieves lease information."]
        async fn lease_time_to_live(
            &self,
            request: tonic::Request<super::LeaseTimeToLiveRequest>,
        ) -> Result<tonic::Response<super::LeaseTimeToLiveResponse>, tonic::Status>;
        #[doc = " LeaseLeases lists all existing leases."]
        async fn lease_leases(
            &self,
            request: tonic::Request<super::LeaseLeasesRequest>,
        ) -> Result<tonic::Response<super::LeaseLeasesResponse>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct LeaseServer<T: Lease> {
        inner: _Inner<T>,
        accept_compression_encodings: (),
        send_compression_encodings: (),
    }
    struct _Inner<T>(Arc<T>);
    impl<T: Lease> LeaseServer<T> {
        pub fn new(inner: T) -> Self {
            let inner = Arc::new(inner);
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
            }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for LeaseServer<T>
    where
        T: Lease,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = Never;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/galadh.Lease/LeaseGrant" => {
                    #[allow(non_camel_case_types)]
                    struct LeaseGrantSvc<T: Lease>(pub Arc<T>);
                    impl<T: Lease> tonic::server::UnaryService<super::LeaseGrantRequest> for LeaseGrantSvc<T> {
                        type Response = super::LeaseGrantResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LeaseGrantRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).lease_grant(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = LeaseGrantSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.Lease/LeaseRevoke" => {
                    #[allow(non_camel_case_types)]
                    struct LeaseRevokeSvc<T: Lease>(pub Arc<T>);
                    impl<T: Lease> tonic::server::UnaryService<super::LeaseRevokeRequest> for LeaseRevokeSvc<T> {
                        type Response = super::LeaseRevokeResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LeaseRevokeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).lease_revoke(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = LeaseRevokeSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.Lease/LeaseKeepAlive" => {
                    #[allow(non_camel_case_types)]
                    struct LeaseKeepAliveSvc<T: Lease>(pub Arc<T>);
                    impl<T: Lease> tonic::server::StreamingService<super::LeaseKeepAliveRequest>
                        for LeaseKeepAliveSvc<T>
                    {
                        type Response = super::LeaseKeepAliveResponse;
                        type ResponseStream = T::LeaseKeepAliveStream;
                        type Future =
                            BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<tonic::Streaming<super::LeaseKeepAliveRequest>>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).lease_keep_alive(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = LeaseKeepAliveSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.Lease/LeaseTimeToLive" => {
                    #[allow(non_camel_case_types)]
                    struct LeaseTimeToLiveSvc<T: Lease>(pub Arc<T>);
                    impl<T: Lease> tonic::server::UnaryService<super::LeaseTimeToLiveRequest>
                        for LeaseTimeToLiveSvc<T>
                    {
                        type Response = super::LeaseTimeToLiveResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LeaseTimeToLiveRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).lease_time_to_live(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = LeaseTimeToLiveSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/galadh.Lease/LeaseLeases" => {
                    #[allow(non_camel_case_types)]
                    struct LeaseLeasesSvc<T: Lease>(pub Arc<T>);
                    impl<T: Lease> tonic::server::UnaryService<super::LeaseLeasesRequest> for LeaseLeasesSvc<T> {
                        type Response = super::LeaseLeasesResponse;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LeaseLeasesRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).lease_leases(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = LeaseLeasesSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec).apply_compression_config(
                            accept_compression_encodings,
                            send_compression_encodings,
                        );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => Box::pin(async move {
                    Ok(http::Response::builder()
                        .status(200)
                        .header("grpc-status", "12")
                        .header("content-type", "application/grpc")
                        .body(empty_body())
                        .unwrap())
                }),
            }
        }
    }
    impl<T: Lease> Clone for LeaseServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
            }
        }
    }
    impl<T: Lease> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Lease> tonic::transport::NamedService for LeaseServer<T> {
        const NAME: &'static str = "galadh.Lease";
    }
}
