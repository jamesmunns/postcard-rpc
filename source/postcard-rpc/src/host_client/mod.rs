//! A postcard-rpc host client
//!
//! This library is meant to be used with the `Dispatch` type and the
//! postcard-rpc wire protocol.

use std::{
    future::Future,
    marker::PhantomData,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use maitake_sync::{
    wait_map::{WaitError, WakeOutcome},
    WaitMap,
};
use postcard::experimental::schema::Schema;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    select,
    sync::mpsc::{Receiver, Sender},
};

use crate::{Endpoint, Key, Topic, WireHeader};

use self::util::Stopper;

#[cfg(all(feature = "raw-nusb", not(target_family = "wasm")))]
mod raw_nusb;

#[cfg(all(feature = "cobs-serial", not(target_family = "wasm")))]
mod serial;

#[cfg(all(feature = "webusb", target_family = "wasm"))]
pub mod webusb;

pub(crate) mod util;

/// Host Error Kind
#[derive(Debug, PartialEq)]
pub enum HostErr<WireErr> {
    /// An error of the user-specified wire error type
    Wire(WireErr),
    /// We got a response that didn't match the expected value or the
    /// user specified wire error type
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
    type Error: std::error::Error; // or std?
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
    type Error: std::error::Error; // or std?
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>>;
}

/// Wire Spawn Interface
///
/// Should be suitable for spawning a task in the host executor.
#[cfg(target_family = "wasm")]
pub trait WireSpawn: 'static {
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
    type Error: std::error::Error; // or std?
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
    type Error: std::error::Error; // or std?
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

/// Wire Spawn Interface
///
/// Should be suitable for spawning a task in the host executor.
#[cfg(not(target_family = "wasm"))]
pub trait WireSpawn: 'static {
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
    out: Sender<RpcFrame>,
    subber: Sender<SubInfo>,
    err_key: Key,
    stopper: Stopper,
    _pd: PhantomData<fn() -> WireErr>,
}

/// # Constructor Methods
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Create a new manually implemented [HostClient].
    ///
    /// This is now deprecated, it is recommended to use [`HostClient::new_with_wire`] instead.
    #[deprecated = "HostClient::new_manual will become private in the future, use HostClient::new_with_wire instead"]
    pub fn new_manual(err_uri_path: &str, outgoing_depth: usize) -> (Self, WireContext) {
        Self::new_manual_priv(err_uri_path, outgoing_depth)
    }

    /// Private method for creating internal context
    pub(crate) fn new_manual_priv(
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> (Self, WireContext) {
        let (tx_pc, rx_pc) = tokio::sync::mpsc::channel(outgoing_depth);
        let (tx_si, rx_si) = tokio::sync::mpsc::channel(outgoing_depth);

        let ctx = Arc::new(HostContext {
            map: WaitMap::new(),
            seq: AtomicU32::new(0),
        });

        let err_key = Key::for_path::<WireErr>(err_uri_path);

        let me = HostClient {
            ctx: ctx.clone(),
            out: tx_pc,
            err_key,
            _pd: PhantomData,
            subber: tx_si.clone(),
            stopper: Stopper::new(),
        };

        let wire = WireContext {
            outgoing: rx_pc,
            incoming: ctx,
            new_subs: rx_si,
        };

        (me, wire)
    }
}

/// # Interface Methods
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
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
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.send_resp_inner::<E>(t);
        select! {
            _ = cancel_fut => Err(HostErr::Closed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::send_resp]
    async fn send_resp_inner<E: Endpoint>(
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
            header: WireHeader {
                key: E::REQ_KEY,
                seq_no,
            },
            body: msg,
        };
        self.out.send(frame).await.map_err(|_| HostErr::Closed)?;
        let ok_resp = self.ctx.map.wait(WireHeader {
            seq_no,
            key: E::RESP_KEY,
        });
        let err_resp = self.ctx.map.wait(WireHeader {
            seq_no,
            key: self.err_key,
        });

        select! {
            o = ok_resp => {
                let resp = o?;
                let r = postcard::from_bytes::<E::Response>(&resp)?;
                Ok(r)
            },
            e = err_resp => {
                let resp = e?;
                let r = postcard::from_bytes::<WireErr>(&resp)?;
                Err(HostErr::Wire(r))
            },
        }
    }

    /// Publish a [Topic] [Message][Topic::Message].
    ///
    /// There is no feedback if the server received our message. If the I/O worker is
    /// closed, an error is returned.
    pub async fn publish<T: Topic>(&self, seq_no: u32, msg: &T::Message) -> Result<(), IoClosed>
    where
        T::Message: Serialize,
    {
        let cancel_fut = self.stopper.wait_stopped();
        let operate_fut = self.publish_inner::<T>(seq_no, msg);
        select! {
            _ = cancel_fut => Err(IoClosed),
            res = operate_fut => res,
        }
    }

    /// Inner function version of [Self::publish]
    async fn publish_inner<T: Topic>(&self, seq_no: u32, msg: &T::Message) -> Result<(), IoClosed>
    where
        T::Message: Serialize,
    {
        let smsg = postcard::to_stdvec(msg).expect("alloc should never fail");
        self.out
            .send(RpcFrame {
                header: WireHeader {
                    key: T::TOPIC_KEY,
                    seq_no,
                },
                body: smsg,
            })
            .await
            .map_err(|_| IoClosed)
    }

    /// Begin listening to a [Topic], receiving a [Subscription] that will give a
    /// stream of [Message][Topic::Message]s.
    ///
    /// If you subscribe to the same topic multiple times, the previous subscription
    /// will be closed (there can be only one).
    ///
    /// Returns an Error if the I/O worker is closed.
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
        self.subber
            .send(SubInfo {
                key: T::TOPIC_KEY,
                tx,
            })
            .await
            .map_err(|_| IoClosed)?;
        Ok(Subscription {
            rx,
            _pd: PhantomData,
        })
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

/// A structure that represents a subscription to the given topic
pub struct Subscription<M> {
    rx: Receiver<RpcFrame>,
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

// Manual Clone impl because WireErr may not impl Clone
impl<WireErr> Clone for HostClient<WireErr> {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            out: self.out.clone(),
            err_key: self.err_key,
            _pd: PhantomData,
            subber: self.subber.clone(),
            stopper: self.stopper.clone(),
        }
    }
}

/// A new subscription that should be accounted for
pub struct SubInfo {
    pub key: Key,
    pub tx: Sender<RpcFrame>,
}

/// Items necessary for implementing a custom I/O Task
pub struct WireContext {
    /// This is a stream of frames that should be placed on the
    /// wire towards the server.
    pub outgoing: Receiver<RpcFrame>,
    /// This shared information contains the WaitMap used for replying to
    /// open requests.
    pub incoming: Arc<HostContext>,
    /// This is a stream of new subscriptions that should be tracked
    pub new_subs: Receiver<SubInfo>,
}

/// A single postcard-rpc frame
pub struct RpcFrame {
    /// The wire header
    pub header: WireHeader,
    /// The serialized message payload
    pub body: Vec<u8>,
}

impl RpcFrame {
    /// Serialize the `RpcFrame` into a Vec of bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = postcard::to_stdvec(&self.header).expect("Alloc should never fail");
        out.extend_from_slice(&self.body);
        out
    }
}

/// Shared context between [HostClient] and the I/O worker task
pub struct HostContext {
    map: WaitMap<WireHeader, Vec<u8>>,
    seq: AtomicU32,
}

/// The I/O worker has closed.
#[derive(Debug)]
pub struct IoClosed;

/// Error for [HostContext::process].
#[derive(Debug, PartialEq)]
pub enum ProcessError {
    /// All [HostClient]s have been dropped, no further requests
    /// will be made and no responses will be processed.
    Closed,
}

impl HostContext {
    /// Like `HostContext::process` but tells you if we processed the message or
    /// nobody wanted it
    pub fn process_did_wake(&self, frame: RpcFrame) -> Result<bool, ProcessError> {
        match self.map.wake(&frame.header, frame.body) {
            WakeOutcome::Woke => Ok(true),
            WakeOutcome::NoMatch(_) => Ok(false),
            WakeOutcome::Closed(_) => Err(ProcessError::Closed),
        }
    }

    /// Process the message, returns Ok if the message was taken or dropped.
    ///
    /// Returns an Err if the map was closed.
    pub fn process(&self, frame: RpcFrame) -> Result<(), ProcessError> {
        if let WakeOutcome::Closed(_) = self.map.wake(&frame.header, frame.body) {
            Err(ProcessError::Closed)
        } else {
            Ok(())
        }
    }
}
