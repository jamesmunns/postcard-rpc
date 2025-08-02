//! A postcard-rpc host client
//!
//! This library is meant to be used with the `Dispatch` type and the
//! postcard-rpc wire protocol.

use core::time::Duration;
use std::{
    collections::HashSet,
    future::Future,
    marker::PhantomData,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
};

use thiserror::Error;

use maitake_sync::{
    wait_map::{WaitError, WakeOutcome},
    WaitMap,
};
use postcard_schema::{schema::owned::OwnedNamedType, Schema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::{
    select,
    sync::{broadcast, mpsc, Mutex},
};
use util::Subscriptions;

use crate::{
    header::{VarHeader, VarKey, VarKeyKind, VarSeq, VarSeqKind},
    standard_icd::{GetAllSchemaDataTopic, GetAllSchemasEndpoint, OwnedSchemaData},
    Endpoint, Key, Topic, TopicDirection,
};

use self::util::Stopper;
pub use crate::host_client::util::HostClientConfig;

#[cfg(all(feature = "raw-nusb", not(target_family = "wasm")))]
mod raw_nusb;

#[cfg(all(feature = "cobs-serial", not(target_family = "wasm")))]
mod serial;

#[cfg(all(feature = "webusb", target_family = "wasm"))]
pub mod webusb;

pub(crate) mod util;

#[cfg(feature = "test-utils")]
pub mod test_channels;

/// Host Error Kind
#[derive(Debug, PartialEq)]
pub enum HostErr<WireErr> {
    /// An error of the user-specified wire error type
    Wire(WireErr),
    /// We got a response that didn't match the expected value or the
    /// user specified wire error type
    ///
    /// This is also (misused) to report when duplicate sequence numbers
    /// in-flight at the same time are detected.
    BadResponse,
    /// Deserialization of the message failed
    Postcard(postcard::Error),
    /// The interface has been closed, and no further messages are possible
    Closed,
}

impl<T> From<postcard::Error> for HostErr<T> {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}

impl<T> From<WaitError> for HostErr<T> {
    fn from(_: WaitError) -> Self {
        Self::Closed
    }
}

/// Wire Transmit Interface
///
/// Responsible for taking a serialized frame (including header and payload),
/// performing any further encoding if necessary, and transmitting to the device.
///
/// Should complete once the message is fully sent (e.g. not just enqueued)
/// if possible.
///
/// All errors are treated as fatal - resolvable or ignorable errors should not
/// be returned to the caller.
#[cfg(target_family = "wasm")]
pub trait WireTx: 'static {
    /// Transmit error type
    type Error: std::error::Error;
    /// Send a single frame
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>>;
}

/// Wire Receive Interface
///
/// Responsible for accumulating a serialized frame (including header and payload),
/// performing any further decoding if necessary, and returning to the caller.
///
/// All errors are treated as fatal - resolvable or ignorable errors should not
/// be returned to the caller.
#[cfg(target_family = "wasm")]
pub trait WireRx: 'static {
    /// Receive error type
    type Error: std::error::Error; // or std?
    /// Receive a single frame
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>>;
}

/// Wire Spawn Interface
///
/// Should be suitable for spawning a task in the host executor.
#[cfg(target_family = "wasm")]
pub trait WireSpawn: 'static {
    /// Spawn a task
    fn spawn(&mut self, fut: impl Future<Output = ()> + 'static);
}

/// Wire Transmit Interface
///
/// Responsible for taking a serialized frame (including header and payload),
/// performing any further encoding if necessary, and transmitting to the device.
///
/// Should complete once the message is fully sent (e.g. not just enqueued)
/// if possible.
///
/// All errors are treated as fatal - resolvable or ignorable errors should not
/// be returned to the caller.
#[cfg(not(target_family = "wasm"))]
pub trait WireTx: Send + 'static {
    /// Transmit error type
    type Error: std::error::Error;
    /// Send a single frame
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Wire Receive Interface
///
/// Responsible for accumulating a serialized frame (including header and payload),
/// performing any further decoding if necessary, and returning to the caller.
///
/// All errors are treated as fatal - resolvable or ignorable errors should not
/// be returned to the caller.
#[cfg(not(target_family = "wasm"))]
pub trait WireRx: Send + 'static {
    /// Receive error type
    type Error: std::error::Error;
    /// Receive a single frame
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

/// Wire Spawn Interface
///
/// Should be suitable for spawning a task in the host executor.
#[cfg(not(target_family = "wasm"))]
pub trait WireSpawn: 'static {
    /// Spawn a task
    fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static);
}

/// The [HostClient] is the primary PC-side interface.
///
/// It is generic over a single type, `WireErr`, which can be used by the
/// embedded system when a request was not understood, or some other error
/// has occurred.
///
/// [HostClient]s can be cloned, and used across multiple tasks/threads.
///
/// There are currently two ways to create one, based on the transport used:
///
/// 1. With raw USB Bulk transfers: [`HostClient::new_raw_nusb()`] (**recommended**)
/// 2. With cobs CDC-ACM transfers: [`HostClient::new_serial_cobs()`]
pub struct HostClient<WireErr> {
    ctx: Arc<HostContext>,
    out: mpsc::Sender<RpcFrame>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    err_key: Key,
    stopper: Stopper,
    seq_kind: VarSeqKind,
    _pd: PhantomData<fn() -> WireErr>,
}

impl<W> core::fmt::Debug for HostClient<W> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostClient").finish_non_exhaustive()
    }
}

/// # Constructor Methods
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Private method for creating internal context
    pub(crate) fn new_manual_priv(config: &HostClientConfig) -> (Self, WireContext) {
        let (tx_pc, rx_pc) = tokio::sync::mpsc::channel(config.outgoing_depth);

        let ctx = Arc::new(HostContext {
            kkind: RwLock::new(VarKeyKind::Key8),
            map: WaitMap::new(),
            seq: AtomicU32::new(0),
            subscription_timeout: config.subscriber_timeout_if_full,
        });

        let err_key = Key::for_path::<WireErr>(config.err_uri_path);

        let me = HostClient {
            ctx: ctx.clone(),
            out: tx_pc,
            err_key,
            _pd: PhantomData,
            subscriptions: Arc::new(Mutex::new(Subscriptions::default())),
            stopper: Stopper::new(),
            seq_kind: config.seq_kind,
        };

        let wire = WireContext {
            outgoing: rx_pc,
            incoming: ctx,
        };

        (me, wire)
    }
}

/// Errors related to retrieving the schema
#[derive(Debug)]
pub enum SchemaError<WireErr> {
    /// Some kind of communication error occurred
    Comms(HostErr<WireErr>),
    /// An error occurred internally. Please open an issue.
    TaskError,
    /// Invalid report data was received, including endpoints or
    /// tasks that referred to unknown types. Please open an issue
    InvalidReportData,
    /// Data was lost while transmitting. If a retry does not solve
    /// this, please open an issue.
    LostData,
}

impl<WireErr> From<UnableToFindType> for SchemaError<WireErr> {
    fn from(_: UnableToFindType) -> Self {
        Self::InvalidReportData
    }
}

/// # Interface Methods
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Obtain a [`SchemaReport`] describing the connected device
    pub async fn get_schema_report(&self) -> Result<SchemaReport, SchemaError<WireErr>> {
        let Ok(mut sub) = self.subscribe_multi::<GetAllSchemaDataTopic>(64).await else {
            return Err(SchemaError::Comms(HostErr::Closed));
        };

        let collect_task = tokio::task::spawn({
            async move {
                let mut got = vec![];
                while let Ok(Ok(val)) =
                    tokio::time::timeout(Duration::from_millis(100), sub.recv()).await
                {
                    got.push(val);
                }
                got
            }
        });
        let trigger_task = self.send_resp::<GetAllSchemasEndpoint>(&()).await;
        let data = collect_task.await;
        let (resp, data) = match (trigger_task, data) {
            (Ok(a), Ok(b)) => (a, b),
            (Ok(_), Err(_)) => return Err(SchemaError::TaskError),
            (Err(e), Ok(_)) => return Err(SchemaError::Comms(e)),
            (Err(e1), Err(_e2)) => return Err(SchemaError::Comms(e1)),
        };
        let mut rpt = SchemaReport::default();
        let mut e_and_t = vec![];

        for d in data {
            match d {
                OwnedSchemaData::Type(d) => {
                    rpt.add_type(d);
                }
                e @ OwnedSchemaData::Endpoint { .. } => e_and_t.push(e),
                t @ OwnedSchemaData::Topic { .. } => e_and_t.push(t),
            }
        }

        for e in e_and_t {
            match e {
                OwnedSchemaData::Type(_) => unreachable!(),
                OwnedSchemaData::Endpoint {
                    path,
                    request_key,
                    response_key,
                } => {
                    rpt.add_endpoint(path, request_key, response_key)?;
                }
                OwnedSchemaData::Topic {
                    path,
                    key,
                    direction,
                } => match direction {
                    TopicDirection::ToServer => rpt.add_topic_in(path, key)?,
                    TopicDirection::ToClient => rpt.add_topic_out(path, key)?,
                },
            }
        }

        let mut data_matches = true;
        data_matches &= resp.endpoints_sent as usize == rpt.endpoints.len();
        data_matches &= resp.topics_in_sent as usize == rpt.topics_in.len();
        data_matches &= resp.topics_out_sent as usize == rpt.topics_out.len();
        data_matches &= resp.errors == 0;

        if data_matches {
            // TODO: filter primitive types out?
            Ok(rpt)
        } else {
            Err(SchemaError::LostData)
        }
    }

    /// Send a message of type [Endpoint::Request][Endpoint] to `path`, and await
    /// a response of type [Endpoint::Response][Endpoint] (or WireErr) to `path`.
    ///
    /// This function will wait potentially forever. Consider using with a timeout.
    pub async fn send_resp<E: Endpoint>(
        &self,
        t: &E::Request,
    ) -> Result<E::Response, HostErr<WireErr>>
    where
        E::Request: Serialize + Schema,
        E::Response: DeserializeOwned + Schema,
    {
        let seq_no = self.ctx.seq.fetch_add(1, Ordering::Relaxed);

        let msg = postcard::to_stdvec(&t).expect("Allocations should not ever fail");
        let frame = RpcFrame {
            // NOTE: send_resp_raw automatically shrinks down key and sequence
            // kinds to the appropriate amount
            header: VarHeader {
                key: VarKey::Key8(E::REQ_KEY),
                seq_no: VarSeq::Seq4(seq_no),
            },
            body: msg,
        };
        let frame = self.send_resp_raw(frame, E::RESP_KEY).await?;
        let r = postcard::from_bytes::<E::Response>(&frame.body)?;
        Ok(r)
    }

    /// Perform an endpoint request/response,but without handling the
    /// Ser/De automatically
    pub async fn send_resp_raw(
        &self,
        mut rqst: RpcFrame,
        resp_key: Key,
    ) -> Result<RpcFrame, HostErr<WireErr>> {
        let cancel_fut = self.stopper.wait_stopped();
        let kkind: VarKeyKind = *self.ctx.kkind.read().unwrap();
        rqst.header.key.shrink_to(kkind);
        let mut resp_key = VarKey::Key8(resp_key);
        let mut err_key = VarKey::Key8(self.err_key);
        resp_key.shrink_to(kkind);
        err_key.shrink_to(kkind);

        // Prepare to receive the reply, BEFORE we send the request.
        // This uses the `enqueue` feature of WaitMap, which makes sure that
        // our receiver is ready to "catch" before we even send the request.
        let ok_resp = self.ctx.map.wait(VarHeader {
            seq_no: rqst.header.seq_no,
            key: resp_key,
        });
        let err_resp = self.ctx.map.wait(VarHeader {
            seq_no: rqst.header.seq_no,
            key: err_key,
        });
        let mut ok_resp = std::pin::pin!(ok_resp);
        let mut err_resp = std::pin::pin!(err_resp);
        let setup_fut: Result<(), WaitError> = async {
            ok_resp.as_mut().enqueue().await?;
            err_resp.as_mut().enqueue().await?;
            Ok(())
        }
        .await;

        // If registering for the response failed, return an error
        if let Err(e) = setup_fut {
            return Err(match e {
                WaitError::Closed => HostErr::Closed,
                WaitError::Duplicate => {
                    tracing::error!("Attempted to register a duplicate wait for a reply. This can happen if sequence numbers are reused.");
                    // TODO: This is the wrong kind of error, but we don't want to report closed.
                    // Fix this in the next breaking change of postcard-rpc, or make HostErr non-exhaustive
                    HostErr::BadResponse
                }

                // These should never happen: NeverAdded and AlreadyConsumed
                _ => {
                    tracing::error!("Internal error setting up reply: {e:?}, closing");
                    self.close();
                    HostErr::Closed
                }
            });
        };

        self.out.send(rqst).await.map_err(|_| HostErr::Closed)?;

        select! {
            _c = cancel_fut => Err(HostErr::Closed),
            o = ok_resp => {
                let (hdr, resp) = o?;
                if hdr.key.kind() != kkind {
                    *self.ctx.kkind.write().unwrap() = hdr.key.kind();
                }
                Ok(RpcFrame { header: hdr, body: resp })
            },
            e = err_resp => {
                let (hdr, resp) = e?;
                if hdr.key.kind() != kkind {
                    *self.ctx.kkind.write().unwrap() = hdr.key.kind();
                }
                let r = postcard::from_bytes::<WireErr>(&resp)?;
                Err(HostErr::Wire(r))
            },
        }
    }

    /// Publish a [Topic] [Message][Topic::Message].
    ///
    /// There is no feedback if the server received our message. If the I/O worker is
    /// closed, an error is returned.
    pub async fn publish<T: Topic>(&self, seq_no: VarSeq, msg: &T::Message) -> Result<(), IoClosed>
    where
        T::Message: Serialize,
    {
        let smsg = postcard::to_stdvec(msg).expect("alloc should never fail");
        let frame = RpcFrame {
            header: VarHeader {
                key: VarKey::Key8(T::TOPIC_KEY),
                seq_no,
            },
            body: smsg,
        };
        self.publish_raw(frame).await
    }

    /// Publish the given raw frame
    pub async fn publish_raw(&self, mut frame: RpcFrame) -> Result<(), IoClosed> {
        let kkind: VarKeyKind = *self.ctx.kkind.read().unwrap();
        frame.header.key.shrink_to(kkind);

        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.out.send(frame);

        select! {
            _ = cancel_fut => Err(IoClosed),
            res = operate_fut => res.map_err(|_| IoClosed),
        }
    }

    ///////////////////////////////////////////////////////////////////////////
    // Subscribe Multi
    ///////////////////////////////////////////////////////////////////////////

    /// Begin listening to a [Topic], receiving a [Subscription] that will give a
    /// stream of [Message][Topic::Message]s. Unlike `subscribe`, multiple subscribers
    /// to the same stream are allowed, and behave as a broadcast channel.
    ///
    /// Returns an Error if the I/O worker is closed.
    pub async fn subscribe_multi<T: Topic>(
        &self,
        depth: usize,
    ) -> Result<MultiSubscription<T::Message>, IoClosed>
    where
        T::Message: DeserializeOwned,
    {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.subscribe_multi_inner::<T>(depth);
        select! {
            _ = cancel_fut => Err(IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::subscribe_multi]
    async fn subscribe_multi_inner<T: Topic>(
        &self,
        depth: usize,
    ) -> Result<MultiSubscription<T::Message>, IoClosed>
    where
        T::Message: DeserializeOwned,
    {
        let rx = {
            let mut guard = self.subscriptions.lock().await;
            if guard.stopped {
                return Err(IoClosed);
            }
            if let Some(entry) = guard
                .broadcast_list
                .iter_mut()
                .find(|(k, _)| *k == T::TOPIC_KEY)
            {
                entry.1.subscribe()
            } else {
                let (tx, rx) = broadcast::channel(depth);
                guard.broadcast_list.push((T::TOPIC_KEY, tx));
                rx
            }
        };
        Ok(MultiSubscription {
            rx,
            _pd: PhantomData,
        })
    }

    /// Subscribe to the given [`Key`], without automatically handling deserialization
    pub async fn subscribe_multi_raw(
        &self,
        key: Key,
        depth: usize,
    ) -> Result<RawMultiSubscription, IoClosed> {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.subscribe_multi_inner_raw(key, depth);
        select! {
            _ = cancel_fut => Err(IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::subscribe]
    async fn subscribe_multi_inner_raw(
        &self,
        key: Key,
        depth: usize,
    ) -> Result<RawMultiSubscription, IoClosed> {
        let rx = {
            let mut guard = self.subscriptions.lock().await;
            if guard.stopped {
                return Err(IoClosed);
            }
            if let Some(entry) = guard.broadcast_list.iter_mut().find(|(k, _)| *k == key) {
                entry.1.subscribe()
            } else {
                let (tx, rx) = broadcast::channel(depth);
                guard.broadcast_list.push((key, tx));
                rx
            }
        };
        Ok(RawMultiSubscription { rx })
    }

    ///////////////////////////////////////////////////////////////////////////
    // Subscribe (Legacy)
    ///////////////////////////////////////////////////////////////////////////

    /// Begin listening to a [Topic], receiving a [Subscription] that will give a
    /// stream of [Message][Topic::Message]s.
    ///
    /// If you subscribe to the same topic multiple times, the previous subscription
    /// will be closed (there can be only one). This does not apply to subscriptions
    /// created with `subscribe_multi`. This also WILL close subscriptions opened by
    /// [`subscribe_exclusive`](Self::subscribe_exclusive).
    ///
    /// Returns an Error if the I/O worker is closed.
    #[deprecated = "In future versions, `subscribe` will be removed. Use `subscribe_multi` or `subscribe_exclusive` instead."]
    pub async fn subscribe<T: Topic>(
        &self,
        depth: usize,
    ) -> Result<Subscription<T::Message>, IoClosed>
    where
        T::Message: DeserializeOwned,
    {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.subscribe_inner::<T>(depth);
        select! {
            _ = cancel_fut => Err(IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::subscribe]
    async fn subscribe_inner<T: Topic>(
        &self,
        depth: usize,
    ) -> Result<Subscription<T::Message>, IoClosed>
    where
        T::Message: DeserializeOwned,
    {
        let (tx, rx) = tokio::sync::mpsc::channel(depth);
        {
            let mut guard = self.subscriptions.lock().await;
            if guard.stopped {
                return Err(IoClosed);
            }
            if let Some(entry) = guard
                .exclusive_list
                .iter_mut()
                .find(|(k, _)| *k == T::TOPIC_KEY)
            {
                if !entry.1.is_closed() {
                    tracing::warn!("replacing subscription for topic path '{}'", T::PATH);
                }
                entry.1 = tx;
            } else {
                guard.exclusive_list.push((T::TOPIC_KEY, tx));
            }
        }
        Ok(Subscription {
            rx,
            _pd: PhantomData,
        })
    }

    /// Subscribe to the given [`Key`], without automatically handling deserialization.
    ///
    /// If you subscribe to the same topic multiple times, the previous subscription
    /// will be closed (there can be only one). This does not apply to subscriptions
    /// created with `subscribe_multi`.
    ///
    /// Returns an Error if the I/O worker is closed.
    #[deprecated = "In future versions, `subscribe_raw` will be removed. Use `subscribe_multi_raw` or `subscribe_exclusive_raw` instead."]
    pub async fn subscribe_raw(&self, key: Key, depth: usize) -> Result<RawSubscription, IoClosed> {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.subscribe_inner_raw(key, depth);
        select! {
            _ = cancel_fut => Err(IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::subscribe_raw]
    async fn subscribe_inner_raw(
        &self,
        key: Key,
        depth: usize,
    ) -> Result<RawSubscription, IoClosed> {
        let (tx, rx) = tokio::sync::mpsc::channel(depth);
        {
            let mut guard = self.subscriptions.lock().await;
            if guard.stopped {
                return Err(IoClosed);
            }
            if let Some(entry) = guard.exclusive_list.iter_mut().find(|(k, _)| *k == key) {
                if !entry.1.is_closed() {
                    tracing::warn!("replacing subscription for raw topic key '{:?}'", key);
                }
                entry.1 = tx;
            } else {
                guard.exclusive_list.push((key, tx));
            }
        }
        Ok(RawSubscription { rx })
    }

    ///////////////////////////////////////////////////////////////////////////
    // Subscribe Exclusive
    ///////////////////////////////////////////////////////////////////////////

    /// Begin listening to a [Topic], receiving a [Subscription] that will give a
    /// stream of [Message][Topic::Message]s.
    ///
    /// If you try to subscribe to the same topic multiple times, this function returns a
    /// [`SubscribeError::AlreadySubscribed`] (there can be only one).
    /// This does not apply to subscriptions created with `subscribe_multi`.
    ///
    /// Returns an Error if the I/O worker is closed.
    pub async fn subscribe_exclusive<T: Topic>(
        &self,
        depth: usize,
    ) -> Result<Subscription<T::Message>, SubscribeError>
    where
        T::Message: DeserializeOwned,
    {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.subscribe_inner_exclusive::<T>(depth);
        select! {
            _ = cancel_fut => Err(SubscribeError::IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::subscribe_exclusive]
    async fn subscribe_inner_exclusive<T: Topic>(
        &self,
        depth: usize,
    ) -> Result<Subscription<T::Message>, SubscribeError>
    where
        T::Message: DeserializeOwned,
    {
        let (tx, rx) = tokio::sync::mpsc::channel(depth);
        {
            let mut guard = self.subscriptions.lock().await;
            if guard.stopped {
                return Err(SubscribeError::IoClosed);
            }
            if let Some(entry) = guard
                .exclusive_list
                .iter_mut()
                .find(|(k, _)| *k == T::TOPIC_KEY)
            {
                if !entry.1.is_closed() {
                    return Err(SubscribeError::AlreadySubscribed);
                }
                entry.1 = tx;
            } else {
                guard.exclusive_list.push((T::TOPIC_KEY, tx));
            }
        }
        Ok(Subscription {
            rx,
            _pd: PhantomData,
        })
    }

    /// Subscribe to the given [`Key`], without automatically handling deserialization.
    ///
    /// If you try to subscribe to the same topic multiple times, this function returns a
    /// [`SubscribeError::AlreadySubscribed`] (there can be only one).
    /// This does not apply to subscriptions created with `subscribe_multi`.
    ///
    /// Returns an Error if the I/O worker is closed.
    pub async fn subscribe_exclusive_raw(
        &self,
        key: Key,
        depth: usize,
    ) -> Result<RawSubscription, SubscribeError> {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.subscribe_inner_exclusive_raw(key, depth);
        select! {
            _ = cancel_fut => Err(SubscribeError::IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::subscribe_exclusive_raw]
    async fn subscribe_inner_exclusive_raw(
        &self,
        key: Key,
        depth: usize,
    ) -> Result<RawSubscription, SubscribeError> {
        let (tx, rx) = tokio::sync::mpsc::channel(depth);
        {
            let mut guard = self.subscriptions.lock().await;
            if guard.stopped {
                return Err(SubscribeError::IoClosed);
            }
            if let Some(entry) = guard.exclusive_list.iter_mut().find(|(k, _)| *k == key) {
                if !entry.1.is_closed() {
                    return Err(SubscribeError::AlreadySubscribed);
                }
                entry.1 = tx;
            } else {
                guard.exclusive_list.push((key, tx));
            }
        }
        Ok(RawSubscription { rx })
    }

    /// Permanently close the connection to the client
    ///
    /// All other HostClients sharing the connection (e.g. created by cloning
    /// a single HostClient) will also stop, and no further communication will
    /// succeed. The in-flight messages will not be flushed.
    ///
    /// This will also signal any I/O worker tasks to halt immediately as well.
    pub fn close(&self) {
        self.stopper.stop()
    }

    /// Has this host client been closed?
    pub fn is_closed(&self) -> bool {
        self.stopper.is_stopped()
    }

    /// Wait for the host client to be closed
    pub async fn wait_closed(&self) {
        self.stopper.wait_stopped().await;
    }
}

/// Like Subscription, but receives Raw frames that are not
/// automatically deserialized
pub struct RawSubscription {
    rx: mpsc::Receiver<RpcFrame>,
}

impl RawSubscription {
    /// Await a message for the given subscription.
    ///
    /// Returns [None]` if the subscription was closed
    pub async fn recv(&mut self) -> Option<RpcFrame> {
        self.rx.recv().await
    }
}

/// A structure that represents a subscription to the given topic
pub struct Subscription<M> {
    rx: mpsc::Receiver<RpcFrame>,
    _pd: PhantomData<M>,
}

impl<M> Subscription<M>
where
    M: DeserializeOwned,
{
    /// Await a message for the given subscription.
    ///
    /// Returns [None]` if the subscription was closed
    pub async fn recv(&mut self) -> Option<M> {
        loop {
            let frame = self.rx.recv().await?;
            if let Ok(m) = postcard::from_bytes(&frame.body) {
                return Some(m);
            }
        }
    }
}

/// Like MultiSubscription, but receives Raw frames that are not
/// automatically deserialized
pub struct RawMultiSubscription {
    rx: broadcast::Receiver<RpcFrame>,
}

impl RawMultiSubscription {
    /// Await a message for the given subscription.
    ///
    /// Returns [None]` if the subscription was closed
    pub async fn recv(&mut self) -> Result<RpcFrame, MultiSubRxError> {
        match self.rx.recv().await {
            Ok(f) => Ok(f),
            Err(broadcast::error::RecvError::Closed) => Err(MultiSubRxError::IoClosed),
            Err(broadcast::error::RecvError::Lagged(n)) => Err(MultiSubRxError::Lagged(n)),
        }
    }
}

/// A structure that represents a subscription to the given topic
pub struct MultiSubscription<M> {
    rx: broadcast::Receiver<RpcFrame>,
    _pd: PhantomData<M>,
}

/// Recv
#[derive(Debug, PartialEq, Error)]
pub enum MultiSubRxError {
    /// The receiver was closed
    #[error("Receiver closed")]
    IoClosed,
    /// Lagged behind, this many messages were lost
    #[error("Lagged behind, lost {0} messages")]
    Lagged(u64),
}

impl<M> MultiSubscription<M>
where
    M: DeserializeOwned,
{
    /// Await a message for the given subscription.
    ///
    /// Returns [None]` if the subscription was closed
    pub async fn recv(&mut self) -> Result<M, MultiSubRxError> {
        loop {
            let frame = match self.rx.recv().await {
                Ok(f) => f,
                Err(broadcast::error::RecvError::Closed) => return Err(MultiSubRxError::IoClosed),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    return Err(MultiSubRxError::Lagged(n))
                }
            };
            if let Ok(m) = postcard::from_bytes(&frame.body) {
                return Ok(m);
            }
        }
    }
}

// Manual Clone impl because WireErr may not impl Clone
impl<WireErr> Clone for HostClient<WireErr> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            out: self.out.clone(),
            err_key: self.err_key,
            _pd: PhantomData,
            subscriptions: self.subscriptions.clone(),
            stopper: self.stopper.clone(),
            seq_kind: self.seq_kind,
        }
    }
}

/// Items necessary for implementing a custom I/O Task
pub struct WireContext {
    /// This is a stream of frames that should be placed on the
    /// wire towards the server.
    pub outgoing: mpsc::Receiver<RpcFrame>,
    /// This shared information contains the WaitMap used for replying to
    /// open requests.
    pub incoming: Arc<HostContext>,
}

/// A single postcard-rpc frame
#[derive(Clone)]
pub struct RpcFrame {
    /// The wire header
    pub header: VarHeader,
    /// The serialized message payload
    pub body: Vec<u8>,
}

impl RpcFrame {
    /// Serialize the `RpcFrame` into a Vec of bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.header.write_to_vec();
        out.extend_from_slice(&self.body);
        out
    }
}

/// Shared context between [HostClient] and the I/O worker task
pub struct HostContext {
    kkind: RwLock<VarKeyKind>,
    map: WaitMap<VarHeader, (VarHeader, Vec<u8>)>,
    seq: AtomicU32,
    subscription_timeout: Duration,
}

impl core::fmt::Debug for HostContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostContext").finish_non_exhaustive()
    }
}

/// The I/O worker has closed.
#[derive(Debug, Error)]
#[error("The I/O worker has closed")]
pub struct IoClosed;

/// The I/O worker has closed.
#[derive(Debug, Error)]
pub enum SubscribeError {
    /// The subscription was already active
    #[error("The subscription was already active")]
    AlreadySubscribed,
    /// The I/O worker has closed.
    #[error("The I/O worker has closed")]
    IoClosed,
}

/// Error for [HostContext::process].
#[derive(Debug, PartialEq, Error)]
pub enum ProcessError {
    /// All [HostClient]s have been dropped, no further requests
    /// will be made and no responses will be processed.
    #[error("All clients have been dropped")]
    Closed,
}

impl HostContext {
    /// Like `HostContext::process` but tells you if we processed the message or
    /// nobody wanted it
    pub fn process_did_wake(&self, frame: RpcFrame) -> Result<bool, ProcessError> {
        match self.map.wake(&frame.header, (frame.header, frame.body)) {
            WakeOutcome::Woke => Ok(true),
            WakeOutcome::NoMatch(_) => Ok(false),
            WakeOutcome::Closed(_) => Err(ProcessError::Closed),
        }
    }

    /// Process the message, returns Ok if the message was taken or dropped.
    ///
    /// Returns an Err if the map was closed.
    pub fn process(&self, frame: RpcFrame) -> Result<(), ProcessError> {
        if let WakeOutcome::Closed(_) = self.map.wake(&frame.header, (frame.header, frame.body)) {
            Err(ProcessError::Closed)
        } else {
            Ok(())
        }
    }
}

/// A report describing the schema spoken by the connected device
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Schema)]
pub struct SchemaReport {
    /// All custom types spoken by the device (on any endpoint or topic),
    /// as well as all primitive types. In the future, primitive types may
    /// be removed.
    pub types: HashSet<OwnedNamedType>,
    /// All incoming (client to server) topics reported by the device
    pub topics_in: Vec<TopicReport>,
    /// All outgoing (server to client) topics reported by the device
    pub topics_out: Vec<TopicReport>,
    /// All endpoints reported by the device
    pub endpoints: Vec<EndpointReport>,
}

impl Default for SchemaReport {
    fn default() -> Self {
        let mut me = Self {
            types: Default::default(),
            topics_in: Default::default(),
            topics_out: Default::default(),
            endpoints: Default::default(),
        };

        // We need to pre-populate all of the types we consider primitives:
        // DataModelType::Bool
        me.add_type(OwnedNamedType::from(<bool as Schema>::SCHEMA));
        // DataModelType::I8
        me.add_type(OwnedNamedType::from(<i8 as Schema>::SCHEMA));
        // DataModelType::U8
        me.add_type(OwnedNamedType::from(<u8 as Schema>::SCHEMA));
        // DataModelType::I16
        me.add_type(OwnedNamedType::from(<i16 as Schema>::SCHEMA));
        // DataModelType::I32
        me.add_type(OwnedNamedType::from(<i32 as Schema>::SCHEMA));
        // DataModelType::I64
        me.add_type(OwnedNamedType::from(<i64 as Schema>::SCHEMA));
        // DataModelType::I128
        me.add_type(OwnedNamedType::from(<i128 as Schema>::SCHEMA));
        // DataModelType::U16
        me.add_type(OwnedNamedType::from(<u16 as Schema>::SCHEMA));
        // DataModelType::U32
        me.add_type(OwnedNamedType::from(<u32 as Schema>::SCHEMA));
        // DataModelType::U64
        me.add_type(OwnedNamedType::from(<u64 as Schema>::SCHEMA));
        // DataModelType::U128
        me.add_type(OwnedNamedType::from(<u128 as Schema>::SCHEMA));
        // // DataModelType::Usize
        // me.add_type(OwnedNamedType::from(<usize as Schema>::SCHEMA));
        // // DataModelType::Isize
        // me.add_type(OwnedNamedType::from(<isize as Schema>::SCHEMA));
        // DataModelType::F32
        me.add_type(OwnedNamedType::from(<f32 as Schema>::SCHEMA));
        // DataModelType::F64
        me.add_type(OwnedNamedType::from(<f64 as Schema>::SCHEMA));
        // DataModelType::Char
        me.add_type(OwnedNamedType::from(<char as Schema>::SCHEMA));
        // DataModelType::String
        me.add_type(OwnedNamedType::from(<String as Schema>::SCHEMA));
        // DataModelType::ByteArray
        me.add_type(OwnedNamedType::from(<Vec<u8> as Schema>::SCHEMA));
        // DataModelType::Unit
        me.add_type(OwnedNamedType::from(<() as Schema>::SCHEMA));
        // DataModelType::Schema
        me.add_type(OwnedNamedType::from(<OwnedNamedType as Schema>::SCHEMA));

        me
    }
}

/// A description of a single Topic
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Schema)]
pub struct TopicReport {
    /// The human readable path of the topic
    pub path: String,
    /// The Key of the topic (which hashes the path and type)
    pub key: Key,
    /// The schema of the type of the message
    pub ty: OwnedNamedType,
}

/// A description of a single Endpoint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Schema)]
pub struct EndpointReport {
    /// The human readable path of the endpoint
    pub path: String,
    /// The Key of the request (which hashes the path and type)
    pub req_key: Key,
    /// The schema of the request type
    pub req_ty: OwnedNamedType,
    /// The Key of the response (which hashes the path and type)
    pub resp_key: Key,
    /// The schema of the response type
    pub resp_ty: OwnedNamedType,
}

/// An error that denotes we were unable to resolve the type used by a given key
#[derive(Debug)]
pub struct UnableToFindType;

impl SchemaReport {
    /// Insert a new type
    pub fn add_type(&mut self, t: OwnedNamedType) {
        self.types.insert(t);
    }

    /// Insert a new incoming (client to server) topic
    ///
    /// Returns an error if we are unable to find the type used for this topic
    pub fn add_topic_in(&mut self, path: String, key: Key) -> Result<(), UnableToFindType> {
        // We need to figure out which type goes with this topic
        for ty in self.types.iter() {
            let calc_key = Key::for_owned_schema_path(&path, ty);
            if calc_key == key {
                self.topics_in.push(TopicReport {
                    path,
                    key,
                    ty: ty.clone(),
                });
                return Ok(());
            }
        }
        Err(UnableToFindType)
    }

    /// Insert a new outgoing (server to client) topic
    ///
    /// Returns an error if we are unable to find the type used for this topic
    pub fn add_topic_out(&mut self, path: String, key: Key) -> Result<(), UnableToFindType> {
        // We need to figure out which type goes with this topic
        for ty in self.types.iter() {
            let calc_key = Key::for_owned_schema_path(&path, ty);
            if calc_key == key {
                self.topics_out.push(TopicReport {
                    path,
                    key,
                    ty: ty.clone(),
                });
                return Ok(());
            }
        }
        Err(UnableToFindType)
    }

    /// Insert a new endpoint
    ///
    /// Returns an error if we are unable to find the type used for the request/response
    pub fn add_endpoint(
        &mut self,
        path: String,
        req_key: Key,
        resp_key: Key,
    ) -> Result<(), UnableToFindType> {
        // We need to figure out which types go with this endpoint
        let mut req_ty = None;
        for ty in self.types.iter() {
            let calc_key = Key::for_owned_schema_path(&path, ty);
            if calc_key == req_key {
                req_ty = Some(ty.clone());
                break;
            }
        }
        let Some(req_ty) = req_ty else {
            return Err(UnableToFindType);
        };

        let mut resp_ty = None;
        for ty in self.types.iter() {
            let calc_key = Key::for_owned_schema_path(&path, ty);
            if calc_key == resp_key {
                resp_ty = Some(ty.clone());
                break;
            }
        }
        let Some(resp_ty) = resp_ty else {
            return Err(UnableToFindType);
        };

        self.endpoints.push(EndpointReport {
            path,
            req_key,
            req_ty,
            resp_key,
            resp_ty,
        });
        Ok(())
    }
}
