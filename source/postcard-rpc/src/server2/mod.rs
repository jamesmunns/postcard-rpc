#![allow(async_fn_in_trait)]

pub mod dispatch_macro;

pub mod impls;

use core::ops::DerefMut;

use postcard_schema::Schema;
use serde::Serialize;

use crate::{header::{VarHeader, VarKey, VarSeq}, Key};

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

pub trait WireTx: Clone {
    type Error: AsWireTxErrorKind;
    async fn send<T: Serialize + ?Sized>(
        &self,
        hdr: VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error>;
    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error>;

    fn sender(&self) -> Sender<Self> {
        Sender::new(self.clone())
    }
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
pub struct Sender<Tx: WireTx> {
    tx: Tx,
}

impl<Tx: WireTx> Sender<Tx> {
    pub fn new(tx: Tx) -> Self {
        Self { tx }
    }

    /// Send a reply for the given endpoint
    #[inline]
    pub async fn reply<E>(&self, seq_no: VarSeq, resp: &E::Response) -> Result<(), Tx::Error>
    where
        E: crate::Endpoint,
        E::Response: Serialize + Schema,
    {
        // TODO: Determine "native" header size
        let wh = VarHeader {
            key: VarKey::Key8(E::RESP_KEY),
            seq_no,
        };
        self.tx.send::<E::Response>(wh, resp).await
    }

    /// Send a reply with the given Key
    ///
    /// This is useful when replying with "unusual" keys, for example Error responses
    /// not tied to any specific Endpoint.
    #[inline]
    pub async fn reply_keyed<T>(&self, seq_no: VarSeq, key: Key, resp: &T) -> Result<(), Tx::Error>
    where
        T: Serialize + Schema,
    {
        let wh = VarHeader {
            key: VarKey::Key8(key),
            seq_no,
        };
        self.tx.send::<T>(wh, resp).await
    }

    /// Publish a Topic message
    #[inline]
    pub async fn publish<T>(&self, seq_no: VarSeq, msg: &T::Message) -> Result<(), Tx::Error>
    where
        T: crate::Topic,
        T::Message: Serialize + Schema,
    {
        let wh = VarHeader {
            key: VarKey::Key8(T::TOPIC_KEY),
            seq_no,
        };
        self.tx.send::<T::Message>(wh, msg).await
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
}

//////////////////////////////////////////////////////////////////////////////
// SERVER
//////////////////////////////////////////////////////////////////////////////

pub struct Server<Tx, Rx, Buf, D>
where
    Tx: WireTx,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
    D: Dispatch2<Tx = Tx>,
{
    tx: Sender<Tx>,
    rx: Rx,
    buf: Buf,
    dis: D,
}

pub enum ServerError<Tx, Rx>
where
    Tx: WireTx,
    Rx: WireRx,
{
    TxFatal(Tx::Error),
    RxFatal(Rx::Error),
}

impl<Tx, Rx, Buf, D> Server<Tx, Rx, Buf, D>
where
    Tx: WireTx,
    Rx: WireRx,
    Buf: DerefMut<Target = [u8]>,
    D: Dispatch2<Tx = Tx>,
{
    pub fn new(tx: &Tx, rx: Rx, buf: Buf, dis: D) -> Self {
        Self {
            tx: Sender { tx: tx.clone() },
            rx,
            buf,
            dis,
        }
    }

    pub async fn run(&mut self) -> ServerError<Tx, Rx> {
        loop {
            let Self {
                tx,
                rx,
                buf,
                dis: d,
            } = self;
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
                continue;
            };
            let fut = d.handle(tx, &hdr, body);
            if let Err(e) = fut.await {
                let kind = e.as_kind();
                match kind {
                    WireTxErrorKind::ConnectionClosed => return ServerError::TxFatal(e),
                    WireTxErrorKind::Other => {}
                }
            }
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
    type SpawnCtxt: 'static;
    fn spawn_ctxt(&mut self) -> Self::SpawnCtxt;
}

pub const fn min_key_needed<const N: usize>(keys: &[Key; N]) -> usize {
    // Can we do it in one?
    {
        let mut keys1 = [0u8; N];
        let mut i = 0;

        while i < keys.len() {
            let [a, b, c, d, e, f, g, h] = keys[i].0;
            keys1[i] = a ^ b ^ c ^ d ^ e ^ f ^ g ^ h;
            i += 1;
        }

        let mut good = true;
        i = 0;

        while i < keys.len() {
            let mut j = i + 1;
            while good && j < keys.len() {
                good &= keys1[i] != keys1[j];
                j += 1;
            }

            i += 1;
        }

        if good {
            return 1;
        }
    }

    // How about two?
    {
        let mut keys2 = [0u16; N];
        let mut i = 0;

        while i < keys.len() {
            let [a, b, c, d, e, f, g, h] = keys[i].0;
            keys2[i] = u16::from_le_bytes([a ^ b ^ c ^ d, e ^ f ^ g ^ h]);
            i += 1;
        }

        let mut good = true;
        i = 0;

        while i < keys.len() {
            let mut j = i + 1;
            while good && j < keys.len() {
                good &= keys2[i] != keys2[j];
                j += 1;
            }

            i += 1;
        }

        if good {
            return 2;
        }
    }

    // How about four?
    {
        let mut keys4 = [0u32; N];
        let mut i = 0;

        while i < keys.len() {
            let [a, b, c, d, e, f, g, h] = keys[i].0;
            keys4[i] = u32::from_le_bytes([a ^ b, c ^ d, e ^ f, g ^ h]);
            i += 1;
        }

        let mut good = true;
        i = 0;

        while i < keys.len() {
            let mut j = i + 1;
            while good && j < keys.len() {
                good &= keys4[i] != keys4[j];
                j += 1;
            }

            i += 1;
        }

        if good {
            return 4;
        }
    }

    // How about eight?
    {
        let mut keys8 = [0u64; N];
        let mut i = 0;

        while i < keys.len() {
            let [a, b, c, d, e, f, g, h] = keys[i].0;
            keys8[i] = u64::from_le_bytes([a, b, c, d, e, f, g, h]);
            i += 1;
        }

        let mut good = true;
        i = 0;

        while i < keys.len() {
            let mut j = i + 1;
            while good && j < keys.len() {
                good &= keys8[i] != keys8[j];
                j += 1;
            }

            i += 1;
        }

        if good {
            return 8;
        }
    }

    panic!("Collision requiring more than 8 bytes!");
}

#[cfg(test)]
mod test {
    use crate::{server2::min_key_needed, Key};

    #[test]
    fn min_test_1() {
        const MIN: usize = min_key_needed(&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]) },
        ]);
        assert_eq!(1, MIN);
    }

    #[test]
    fn min_test_2() {
        const MIN: usize = min_key_needed(&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]) },
        ]);
        assert_eq!(2, MIN);
    }

    #[test]
    fn min_test_4() {
        const MIN: usize = min_key_needed(&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01]) },
        ]);
        assert_eq!(4, MIN);
    }

    #[test]
    fn min_test_8() {
        const MIN: usize = min_key_needed(&[
            unsafe { Key::from_bytes([0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]) },
            unsafe { Key::from_bytes([0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01]) },
        ]);
        assert_eq!(8, MIN);
    }
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
