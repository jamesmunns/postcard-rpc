//! Definitions of a postcard-rpc Server
//!
//! The Server role is responsible for accepting endpoint requests, issuing
//! endpoint responses, receiving client topic messages, and sending server
//! topic messages
//!
//! ## Impls
//!
//! It is intended to allow postcard-rpc servers to be implemented for many
//! different transport types, as well as many different operating environments.
//!
//! Examples of impls include:
//!
//! * A no-std impl using embassy and embassy-usb to provide transport over USB
//! * A std impl using Tokio channels to provide transport for testing
//!
//! Impls are expected to implement three traits:
//!
//! * [`WireTx`]: how the server sends frames to the client
//! * [`WireRx`]: how the server receives frames from the client
//! * [`WireSpawn`]: how the server spawns worker tasks for certain handlers

#![allow(async_fn_in_trait)]

#[doc(hidden)]
pub mod dispatch_macro;

pub mod impls;

use core::{fmt::Arguments, ops::DerefMut};

use postcard_schema::Schema;
use serde::Serialize;

use crate::{
    header::{VarHeader, VarKey, VarKeyKind, VarSeq},
    DeviceMap, Key, TopicDirection,
};

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

/// This trait defines how the server sends frames to the client
pub trait WireTx {
    /// The error type of this connection.
    ///
    /// For simple cases, you can use [`WireTxErrorKind`] directly. You can also
    /// use your own custom type that implements [`AsWireTxErrorKind`].
    type Error: AsWireTxErrorKind;

    /// Wait for the connection to be established
    ///
    /// Should be implemented for connection oriented wire protocols
    async fn wait_connection(&self) {}

    /// Send a single frame to the client, returning when send is complete.
    async fn send<T: Serialize + ?Sized>(&self, hdr: VarHeader, msg: &T)
        -> Result<(), Self::Error>;

    /// Send a single frame to the client, without handling serialization
    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error>;

    /// Send a logging message on the [`LoggingTopic`][crate::standard_icd::LoggingTopic]
    ///
    /// This message is simpler as it does not do any formatting
    async fn send_log_str(&self, kkind: VarKeyKind, s: &str) -> Result<(), Self::Error>;

    /// Send a logging message on the [`LoggingTopic`][crate::standard_icd::LoggingTopic]
    ///
    /// This version formats to the outgoing buffer
    async fn send_log_fmt<'a>(
        &self,
        kkind: VarKeyKind,
        a: Arguments<'a>,
    ) -> Result<(), Self::Error>;
}

/// The base [`WireTx`] Error Kind
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum WireTxErrorKind {
    /// The connection has been closed, and is unlikely to succeed until
    /// the connection is re-established. This will cause the Server run
    /// loop to terminate.
    ConnectionClosed,
    /// Other unspecified errors
    Other,
    /// Timeout (WireTx impl specific) reached
    Timeout,
}

/// A conversion trait to convert a user error into a base Kind type
pub trait AsWireTxErrorKind {
    /// Convert the error type into a base type
    fn as_kind(&self) -> WireTxErrorKind;
}

impl AsWireTxErrorKind for WireTxErrorKind {
    #[inline]
    fn as_kind(&self) -> WireTxErrorKind {
        *self
    }
}

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

/// This trait defines how to receive a single frame from a client
pub trait WireRx {
    /// The error type of this connection.
    ///
    /// For simple cases, you can use [`WireRxErrorKind`] directly. You can also
    /// use your own custom type that implements [`AsWireRxErrorKind`].
    type Error: AsWireRxErrorKind;

    /// Wait for the connection to be established
    ///
    /// Should be implemented for connection oriented wire protocols
    async fn wait_connection(&mut self) {}

    /// Receive a single frame
    ///
    /// On success, the portion of `buf` that contains a single frame is returned.
    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error>;
}

/// The base [`WireRx`] Error Kind
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum WireRxErrorKind {
    /// The connection has been closed, and is unlikely to succeed until
    /// the connection is re-established. This will cause the Server run
    /// loop to terminate.
    ConnectionClosed,
    /// The received message was too large for the server to handle
    ReceivedMessageTooLarge,
    /// Other message kinds
    Other,
}

/// A conversion trait to convert a user error into a base Kind type
pub trait AsWireRxErrorKind {
    /// Convert the error type into a base type
    fn as_kind(&self) -> WireRxErrorKind;
}

impl AsWireRxErrorKind for WireRxErrorKind {
    #[inline]
    fn as_kind(&self) -> WireRxErrorKind {
        *self
    }
}

//////////////////////////////////////////////////////////////////////////////
// SPAWN
//////////////////////////////////////////////////////////////////////////////

/// A trait to assist in spawning a handler task
///
/// This trait is weird, and mostly exists to abstract over how "normal" async
/// executors like tokio spawn tasks, taking a future, and how unusual async
/// executors like embassy spawn tasks, taking a task token that maps to static
/// storage
pub trait WireSpawn: Clone {
    /// An error type returned when spawning fails. If this cannot happen,
    /// [`Infallible`][core::convert::Infallible] can be used.
    type Error;
    /// The context used for spawning a task.
    ///
    /// For example, in tokio this is `()`, and in embassy this is `Spawner`.
    type Info;

    /// Retrieve [`Self::Info`]
    fn info(&self) -> &Self::Info;
}

//////////////////////////////////////////////////////////////////////////////
// SENDER (wrapper of WireTx)
//////////////////////////////////////////////////////////////////////////////

/// The [`Sender`] type wraps a [`WireTx`] impl, and provides higher level functionality
/// over it
#[derive(Clone)]
pub struct Sender<Tx: WireTx> {
    tx: Tx,
    kkind: VarKeyKind,
}

impl<Tx: WireTx> Sender<Tx> {
    /// Create a new Sender
    ///
    /// Takes a [`WireTx`] impl, as well as the [`VarKeyKind`] used when sending messages
    /// to the client.
    ///
    /// `kkind` should usually come from [`Dispatch::min_key_len()`].
    pub fn new(tx: Tx, kkind: VarKeyKind) -> Self {
        Self { tx, kkind }
    }

    /// Send a reply for the given endpoint
    #[inline]
    pub async fn reply<E>(&self, seq_no: VarSeq, resp: &E::Response) -> Result<(), Tx::Error>
    where
        E: crate::Endpoint,
        E::Response: Serialize + Schema,
    {
        let mut key = VarKey::Key8(E::RESP_KEY);
        key.shrink_to(self.kkind);
        let wh = VarHeader { key, seq_no };
        self.tx.send::<E::Response>(wh, resp).await
    }

    /// Send a reply with the given Key
    ///
    /// This is useful when replying with "unusual" keys, for example Error responses
    /// not tied to any specific Endpoint.
    #[inline]
    pub async fn reply_keyed<T>(&self, seq_no: VarSeq, key: Key, resp: &T) -> Result<(), Tx::Error>
    where
        T: ?Sized,
        T: Serialize + Schema,
    {
        let mut key = VarKey::Key8(key);
        key.shrink_to(self.kkind);
        let wh = VarHeader { key, seq_no };
        self.tx.send::<T>(wh, resp).await
    }

    /// Publish a Topic message
    #[inline]
    pub async fn publish<T>(&self, seq_no: VarSeq, msg: &T::Message) -> Result<(), Tx::Error>
    where
        T: ?Sized,
        T: crate::Topic,
        T::Message: Serialize + Schema,
    {
        let mut key = VarKey::Key8(T::TOPIC_KEY);
        key.shrink_to(self.kkind);
        let wh = VarHeader { key, seq_no };
        self.tx.send::<T::Message>(wh, msg).await
    }

    /// Log a `str` directly to the [`LoggingTopic`][crate::standard_icd::LoggingTopic]
    #[inline]
    pub async fn log_str(&self, msg: &str) -> Result<(), Tx::Error> {
        self.tx.send_log_str(self.kkind, msg).await
    }

    /// Format a message to the [`LoggingTopic`][crate::standard_icd::LoggingTopic]
    #[inline]
    pub async fn log_fmt(&self, msg: Arguments<'_>) -> Result<(), Tx::Error> {
        self.tx.send_log_fmt(self.kkind, msg).await
    }

    /// Send a single error message
    pub async fn error(
        &self,
        seq_no: VarSeq,
        error: crate::standard_icd::WireError,
    ) -> Result<(), Tx::Error> {
        self.reply_keyed(seq_no, crate::standard_icd::ERROR_KEY, &error)
            .await
    }

    /// Implements the [`GetAllSchemasEndpoint`][crate::standard_icd::GetAllSchemasEndpoint] endpoint
    pub async fn send_all_schemas(
        &self,
        hdr: &VarHeader,
        device_map: &DeviceMap,
    ) -> Result<(), Tx::Error> {
        #[cfg(feature = "use-std")]
        use crate::standard_icd::OwnedSchemaData as SchemaData;
        #[cfg(not(feature = "use-std"))]
        use crate::standard_icd::SchemaData;
        use crate::standard_icd::{GetAllSchemaDataTopic, GetAllSchemasEndpoint, SchemaTotals};

        let mut msg_ctr = 0;
        let mut err_ctr = 0;

        // First, send all types
        for ty in device_map.types {
            let res = self
                .publish::<GetAllSchemaDataTopic>(
                    VarSeq::Seq2(msg_ctr),
                    &SchemaData::Type((*ty).into()),
                )
                .await;
            if res.is_err() {
                err_ctr += 1;
            };
            msg_ctr += 1;
        }

        // Then all endpoints
        for ep in device_map.endpoints {
            let res = self
                .publish::<GetAllSchemaDataTopic>(
                    VarSeq::Seq2(msg_ctr),
                    &SchemaData::Endpoint {
                        path: ep.0.into(),
                        request_key: ep.1,
                        response_key: ep.2,
                    },
                )
                .await;
            if res.is_err() {
                err_ctr += 1;
            }

            msg_ctr += 1;
        }

        // Then output topics
        for to in device_map.topics_out {
            let res = self
                .publish::<GetAllSchemaDataTopic>(
                    VarSeq::Seq2(msg_ctr),
                    &SchemaData::Topic {
                        direction: TopicDirection::ToClient,
                        path: to.0.into(),
                        key: to.1,
                    },
                )
                .await;
            if res.is_err() {
                err_ctr += 1;
            }
            msg_ctr += 1;
        }

        // Then input topics
        for ti in device_map.topics_in {
            let res = self
                .publish::<GetAllSchemaDataTopic>(
                    VarSeq::Seq2(msg_ctr),
                    &SchemaData::Topic {
                        direction: TopicDirection::ToServer,
                        path: ti.0.into(),
                        key: ti.1,
                    },
                )
                .await;
            if res.is_err() {
                err_ctr += 1;
            }
            msg_ctr += 1;
        }

        // Finally, reply with the totals
        self.reply::<GetAllSchemasEndpoint>(
            hdr.seq_no,
            &SchemaTotals {
                types_sent: device_map.types.len() as u32,
                endpoints_sent: device_map.endpoints.len() as u32,
                topics_in_sent: device_map.topics_in.len() as u32,
                topics_out_sent: device_map.topics_out.len() as u32,
                errors: err_ctr,
            },
        )
        .await?;

        Ok(())
    }
}

//////////////////////////////////////////////////////////////////////////////
// SERVER
//////////////////////////////////////////////////////////////////////////////

/// The [`Server`] is the main interface for handling communication
pub struct Server<Tx, Rx, Buf, D>
where
    Tx: WireTx,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
    D: Dispatch<Tx = Tx>,
{
    tx: Sender<Tx>,
    rx: Rx,
    buf: Buf,
    dis: D,
}

/// A type representing the different errors [`Server::run()`] may return
pub enum ServerError<Tx, Rx>
where
    Tx: WireTx,
    Rx: WireRx,
{
    /// A fatal error occurred with the [`WireTx::send()`] implementation
    TxFatal(Tx::Error),
    /// A fatal error occurred with the [`WireRx::receive()`] implementation
    RxFatal(Rx::Error),
}

impl<Tx, Rx, Buf, D> Server<Tx, Rx, Buf, D>
where
    Tx: WireTx,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
    D: Dispatch<Tx = Tx>,
{
    /// Create a new Server
    ///
    /// Takes:
    ///
    /// * a [`WireTx`] impl for sending
    /// * a [`WireRx`] impl for receiving
    /// * a buffer used for receiving frames
    /// * The user provided dispatching method, usually generated by [`define_dispatch!()`][crate::define_dispatch]
    /// * a [`VarKeyKind`], which controls the key sizes sent by the [`WireTx`] impl
    pub fn new(tx: Tx, rx: Rx, buf: Buf, dis: D, kkind: VarKeyKind) -> Self {
        Self {
            tx: Sender { tx, kkind },
            rx,
            buf,
            dis,
        }
    }

    /// Run until a fatal error occurs
    ///
    /// The server will receive frames, and dispatch them. When a fatal error occurs,
    /// this method will return with the fatal error.
    ///
    /// The caller may decide to wait until a connection is re-established, reset any
    /// state, or immediately begin re-running.
    pub async fn run(&mut self) -> ServerError<Tx, Rx> {
        loop {
            let Self {
                tx,
                rx,
                buf,
                dis: d,
            } = self;
            rx.wait_connection().await;
            tx.tx.wait_connection().await;
            let used = match rx.receive(buf).await {
                Ok(u) => u,
                Err(e) => {
                    let kind = e.as_kind();
                    match kind {
                        WireRxErrorKind::ConnectionClosed => return ServerError::RxFatal(e),
                        WireRxErrorKind::ReceivedMessageTooLarge => continue,
                        WireRxErrorKind::Other => continue,
                    }
                }
            };
            let Some((hdr, body)) = VarHeader::take_from_slice(used) else {
                // TODO: send a nak on badly formed messages? We don't have
                // much to say because we don't have a key or seq no or anything
                continue;
            };
            let fut = d.handle(tx, &hdr, body);
            if let Err(e) = fut.await {
                let kind = e.as_kind();
                match kind {
                    WireTxErrorKind::ConnectionClosed => return ServerError::TxFatal(e),
                    WireTxErrorKind::Other => {}
                    WireTxErrorKind::Timeout => return ServerError::TxFatal(e),
                }
            }
        }
    }
}

impl<Tx, Rx, Buf, D> Server<Tx, Rx, Buf, D>
where
    Tx: WireTx + Clone,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
    D: Dispatch<Tx = Tx>,
{
    /// Get a copy of the [`Sender`] to pass to tasks that need it
    pub fn sender(&self) -> Sender<Tx> {
        self.tx.clone()
    }
}

//////////////////////////////////////////////////////////////////////////////
// DISPATCH TRAIT
//////////////////////////////////////////////////////////////////////////////

/// The dispatch trait handles an incoming endpoint or topic message
///
/// The implementations of this trait are typically implemented by the
/// [`define_dispatch!`][crate::define_dispatch] macro.
pub trait Dispatch {
    /// The [`WireTx`] impl used by this dispatcher
    type Tx: WireTx;

    /// The minimum key length required to avoid hash collisions
    fn min_key_len(&self) -> VarKeyKind;

    /// Handle a single incoming frame (endpoint or topic), and dispatch appropriately
    async fn handle(
        &mut self,
        tx: &Sender<Self::Tx>,
        hdr: &VarHeader,
        body: &[u8],
    ) -> Result<(), <Self::Tx as WireTx>::Error>;
}

//////////////////////////////////////////////////////////////////////////////
// SPAWNCONTEXT TRAIT
//////////////////////////////////////////////////////////////////////////////

/// A conversion trait for taking the Context and making a SpawnContext
///
/// This is necessary if you use the `spawn` variant of `define_dispatch!`.
pub trait SpawnContext {
    /// The spawn context type
    type SpawnCtxt: 'static;
    /// A method to convert the regular context into [`Self::SpawnCtxt`]
    fn spawn_ctxt(&mut self) -> Self::SpawnCtxt;
}

// Hilarious quadruply nested loop. Hope our lists are relatively small!
macro_rules! keycheck {
    (
        $lists:ident;
        $($num:literal => $func:ident;)*
    ) => {
        $(
            {
                let mut i = 0;
                let mut good = true;
                // For each list...
                'dupe: while i < $lists.len() {
                    let ilist = $lists[i];
                    let mut j = 0;
                    // And for each key in the list
                    while j < ilist.len() {
                        let jkey = ilist[j];
                        let akey = $func(jkey);

                        //
                        // We now start checking against items later in the lists...
                        //

                        // For each list (starting with the one we are on)
                        let mut x = i;
                        while x < $lists.len() {
                            // For each item...
                            //
                            // Note that for the STARTING list we continue where we started,
                            // but on subsequent lists start from the beginning
                            let xlist = $lists[x];
                            let mut y = if x == i {
                                j + 1
                            } else {
                                0
                            };

                            while y < xlist.len() {
                                let ykey = xlist[y];
                                let bkey = $func(ykey);

                                if akey == bkey {
                                    good = false;
                                    break 'dupe;
                                }
                                y += 1;
                            }
                            x += 1;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                if good {
                    return $num;
                }
            }
        )*
    };
}

/// Calculates at const time the minimum number of bytes (1, 2, 4, or 8) to avoid
/// hash collisions in the lists of keys provided.
///
/// If there are any duplicates, this function will panic at compile time. Otherwise,
/// this function will return 1, 2, 4, or 8.
///
/// This function takes a very dumb "brute force" approach, that is of the order
/// `O(4 * N^2 * M^2)`, where `N` is `lists.len()`, and `M` is the length of each
/// sub-list. It is not recommended to call this outside of const context.
pub const fn min_key_needed(lists: &[&[Key]]) -> usize {
    const fn one(key: Key) -> u8 {
        crate::Key1::from_key8(key).0
    }
    const fn two(key: Key) -> u16 {
        u16::from_le_bytes(crate::Key2::from_key8(key).0)
    }
    const fn four(key: Key) -> u32 {
        u32::from_le_bytes(crate::Key4::from_key8(key).0)
    }
    const fn eight(key: Key) -> u64 {
        u64::from_le_bytes(key.to_bytes())
    }

    keycheck! {
        lists;
        1 => one;
        2 => two;
        4 => four;
        8 => eight;
    };

    panic!("Collision requiring more than 8 bytes!");
}

#[cfg(test)]
mod test {
    use crate::{server::min_key_needed, Key};

    #[test]
    fn min_test_1() {
        const MINA: usize = min_key_needed(&[&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]) },
        ]]);
        assert_eq!(1, MINA);

        const MINB: usize = min_key_needed(&[
            &[unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) }],
            &[unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]) }],
        ]);
        assert_eq!(1, MINB);
    }

    #[test]
    fn min_test_2() {
        const MINA: usize = min_key_needed(&[&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]) },
        ]]);
        assert_eq!(2, MINA);
        const MINB: usize = min_key_needed(&[
            &[unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) }],
            &[unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]) }],
        ]);
        assert_eq!(2, MINB);
    }

    #[test]
    fn min_test_4() {
        const MINA: usize = min_key_needed(&[&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01]) },
        ]]);
        assert_eq!(4, MINA);
        const MINB: usize = min_key_needed(&[
            &[unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) }],
            &[unsafe { Key::from_bytes([0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01]) }],
        ]);
        assert_eq!(4, MINB);
    }

    #[test]
    fn min_test_8() {
        const MINA: usize = min_key_needed(&[&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01]) },
        ]]);
        assert_eq!(8, MINA);
        const MINB: usize = min_key_needed(&[
            &[unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) }],
            &[unsafe { Key::from_bytes([0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01]) }],
        ]);
        assert_eq!(8, MINB);
    }
}
