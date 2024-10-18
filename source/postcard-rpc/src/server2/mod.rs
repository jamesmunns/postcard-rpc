#![allow(async_fn_in_trait)]

pub mod dispatch_macro;

pub mod impls;

use core::ops::DerefMut;

use postcard_schema::Schema;
use serde::Serialize;

use crate::{headered::extract_header_from_bytes, Key, WireHeader};

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

pub trait WireTx: Clone {
    type Error: AsWireTxErrorKind;
    async fn send<T: Serialize + ?Sized>(
        &self,
        hdr: WireHeader,
        msg: &T,
    ) -> Result<(), Self::Error>;
    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum WireTxErrorKind {
    ConnectionClosed,
    Other,
}

pub trait AsWireTxErrorKind {
    fn as_kind(&self) -> WireTxErrorKind;
}

impl AsWireTxErrorKind for WireTxErrorKind {
    fn as_kind(&self) -> WireTxErrorKind {
        *self
    }
}

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

pub trait WireRx {
    type Error: AsWireRxErrorKind;
    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error>;
}

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum WireRxErrorKind {
    ConnectionClosed,
    ReceivedMessageTooLarge,
    Other,
}

pub trait AsWireRxErrorKind {
    fn as_kind(&self) -> WireRxErrorKind;
}

impl AsWireRxErrorKind for WireRxErrorKind {
    fn as_kind(&self) -> WireRxErrorKind {
        *self
    }
}

//////////////////////////////////////////////////////////////////////////////
// SPAWN
//////////////////////////////////////////////////////////////////////////////

pub trait WireSpawn: Clone {
    type Error;
    type Info;
    fn info(&self) -> &Self::Info;
}

//////////////////////////////////////////////////////////////////////////////
// OUTPUTTER (wrapper of WireTx)
//////////////////////////////////////////////////////////////////////////////

// Needs a better name
#[derive(Clone)]
pub struct Outputter<Tx: WireTx> {
    tx: Tx,
}

impl<Tx: WireTx> Outputter<Tx> {
    /// Send a reply for the given endpoint
    #[inline]
    pub async fn reply<E>(&self, seq_no: u32, resp: &E::Response) -> Result<(), Tx::Error>
    where
        E: crate::Endpoint,
        E::Response: Serialize + Schema,
    {
        let wh = WireHeader {
            key: E::RESP_KEY,
            seq_no,
        };
        self.tx.send::<E::Response>(wh, resp).await
    }

    /// Send a reply with the given Key
    ///
    /// This is useful when replying with "unusual" keys, for example Error responses
    /// not tied to any specific Endpoint.
    #[inline]
    pub async fn reply_keyed<T>(&self, seq_no: u32, key: Key, resp: &T) -> Result<(), Tx::Error>
    where
        T: Serialize + Schema,
    {
        let wh = WireHeader { key, seq_no };
        self.tx.send::<T>(wh, resp).await
    }

    /// Publish a Topic message
    #[inline]
    pub async fn publish<T>(&self, seq_no: u32, msg: &T::Message) -> Result<(), Tx::Error>
    where
        T: crate::Topic,
        T::Message: Serialize + Schema,
    {
        let wh = WireHeader {
            key: T::TOPIC_KEY,
            seq_no,
        };
        self.tx.send::<T::Message>(wh, msg).await
    }

    /// Send a single error message
    pub async fn error(&self, seq_no: u32, error: crate::standard_icd::WireError) {
        // If we get an error while sending an error, welp there's not much we can do
        let _ = self
            .reply_keyed(seq_no, crate::standard_icd::ERROR_KEY, &error)
            .await;
    }
}

//////////////////////////////////////////////////////////////////////////////
// SERVER
//////////////////////////////////////////////////////////////////////////////

pub struct Server<Tx, Rx, Buf>
where
    Tx: WireTx,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
{
    tx: Outputter<Tx>,
    rx: Rx,
    buf: Buf,
}

pub enum ServerError<Tx, Rx>
where
    Tx: WireTx,
    Rx: WireRx,
{
    TxFatal(Tx::Error),
    RxFatal(Rx::Error),
}

impl<Tx, Rx, Buf> Server<Tx, Rx, Buf>
where
    Tx: WireTx,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
{
    pub fn new(tx: &Tx, rx: Rx, buf: Buf) -> Self {
        Self {
            tx: Outputter { tx: tx.clone() },
            rx,
            buf,
        }
    }

    pub async fn run<D: Dispatch2<Tx = Tx>>(&mut self, mut d: D) -> ServerError<Tx, Rx> {
        loop {
            let Self { tx, rx, buf } = self;
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
            let Ok((hdr, body)) = extract_header_from_bytes(used) else {
                continue;
            };
            let fut = d.handle(tx, &hdr, body);
            let Ok(y) = fut.await else {
                continue;
            };
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// DISPATCH TRAIT
//////////////////////////////////////////////////////////////////////////////

pub trait Dispatch2 {
    type Tx: WireTx;
    async fn handle(
        &mut self,
        tx: &Outputter<Self::Tx>,
        hdr: &WireHeader,
        body: &[u8],
    ) -> Result<(), ()>;
}

//////////////////////////////////////////////////////////////////////////////
// TODO KEY STUFF
//////////////////////////////////////////////////////////////////////////////

// pub struct Key8(pub [u8; 8]);
// pub struct Key4(pub [u8; 4]);
// pub struct Key2(pub [u8; 2]);
// pub struct Key1(pub u8);

// impl Key1 {
//     const fn from_key2(value: Key2) -> Self {
//         let [a, b] = value.0;
//         Self(a ^ b)
//     }

//     const fn from_key4(value: Key4) -> Self {
//         let [a, b, c, d] = value.0;
//         Self(a ^ b ^ c ^ d)
//     }

//     const fn from_key8(value: Key8) -> Self {
//         let [a, b, c, d, e, f, g, h] = value.0;
//         Self(a ^ b ^ c ^ d ^ e ^ f ^ g ^ h)
//     }
// }

// impl Key2 {
//     const fn from_key4(value: Key4) -> Self {
//         let [a, b, c, d] = value.0;
//         Self([a ^ b,  c ^ d])
//     }

//     const fn from_key8(value: Key8) -> Self {
//         let [a, b, c, d, e, f, g, h] = value.0;
//         Self([a ^ b ^ c ^ d, e ^ f ^ g ^ h])
//     }
// }

// impl Key4 {
//     const fn from_key8(value: Key8) -> Self {
//         let [a, b, c, d, e, f, g, h] = value.0;
//         Self([a ^ b, c ^ d, e ^ f, g ^ h])
//     }
// }

// impl From<Key2> for Key1 {
//     fn from(value: Key2) -> Self {
//         Self::from_key2(value)
//     }
// }

// impl From<Key4> for Key1 {
//     fn from(value: Key4) -> Self {
//         Self::from_key4(value)
//     }
// }

// impl From<Key8> for Key1 {
//     fn from(value: Key8) -> Self {
//         Self::from_key8(value)
//     }
// }

// impl From<Key4> for Key2 {
//     fn from(value: Key4) -> Self {
//         Self::from_key4(value)
//     }
// }

// impl From<Key8> for Key2 {
//     fn from(value: Key8) -> Self {
//         Self::from_key8(value)
//     }
// }

// impl From<Key8> for Key4 {
//     fn from(value: Key8) -> Self {
//         Self::from_key8(value)
//     }
// }
