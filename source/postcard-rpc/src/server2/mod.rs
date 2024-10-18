#![allow(async_fn_in_trait)]

#[cfg(feature = "embassy-usb-0_3-server")]
pub mod embassy_usb_v0_3;

pub mod dispatch_macro;

use core::future::Future;

use postcard_schema::Schema;
use serde::Serialize;

use crate::{headered::extract_header_from_bytes, Key, WireHeader};

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

pub trait WireTx: Clone {
    type Error;
    async fn send<T: Serialize + ?Sized>(
        &self,
        hdr: WireHeader,
        msg: &T,
    ) -> Result<(), Self::Error>;
    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error>;
}

pub trait WireRx {
    type Error;
    async fn receive<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error>;
}

pub trait WireSpawn: Clone {
    type Error;
    type Info;
    fn info(&self) -> &Self::Info;
}

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

pub struct Server<'a, Tx, Rx>
where
    Tx: WireTx,
    Rx: WireRx,
{
    tx: Outputter<Tx>,
    rx: Rx,
    buf: &'a mut [u8],
}

pub trait Dispatch2 {
    type Tx: WireTx;
    async fn handle(
        &mut self,
        tx: &Outputter<Self::Tx>,
        hdr: &WireHeader,
        body: &[u8],
    ) -> Result<(), ()>;
}

impl<'a, Tx, Rx> Server<'a, Tx, Rx>
where
    Tx: WireTx,
    Rx: WireRx,
{
    pub fn new(tx: &Tx, rx: Rx, buf: &'a mut [u8]) -> Self {
        Self {
            tx: Outputter { tx: tx.clone() },
            rx,
            buf,
        }
    }

    pub async fn run<D: Dispatch2<Tx = Tx>>(&mut self, mut d: D) {
        loop {
            let Self { tx, rx, buf } = self;
            let Ok(used) = self.rx.receive(buf).await else {
                continue;
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

// #[cfg(all(test, feature = "use-std"))]
// mod test {
//     use core::{future::Future, marker::PhantomData};

//     use super::{Dispatch2, Outputter, Server, WireRx, WireSpawn, WireTx};

//     #[derive(Clone)]
//     struct FakeWireTx;
//     struct FakeWireRx;
//     #[derive(Clone)]
//     struct FakeWireSpawn;

//     impl WireTx for FakeWireTx {
//         type Error = ();

//         async fn send<T: serde::Serialize + ?Sized>(
//             &self,
//             _hdr: crate::WireHeader,
//             _msg: &T,
//         ) -> Result<(), Self::Error> {
//             Ok(())
//         }

//         async fn send_raw(&self, _buf: &[u8]) -> Result<(), Self::Error> {
//             Ok(())
//         }
//     }

//     impl WireRx for FakeWireRx {
//         type Error = ();

//         async fn receive<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
//             Ok(buf)
//         }
//     }

//     #[tokio::test]
//     async fn smoke() {
//         let mut buf = [0u8; 512];
//         let mut x = Server::new(&FakeWireTx, FakeWireRx, &mut buf);
//         let disp = FakeDispatch::<FakeWireTx>::new();
//         core::mem::drop(x.run(disp));
//     }

//     struct FakeDispatch<Tx: WireTx> {
//         _pdt: PhantomData<fn() -> Tx>,
//     }

//     impl<Tx: WireTx> FakeDispatch<Tx> {
//         pub fn new() -> Self {
//             Self {
//                 _pdt: PhantomData,
//             }
//         }
//     }

//     impl<Tx: WireTx> Dispatch2 for FakeDispatch<Tx> {
//         type Tx = Tx;

//         async fn handle(
//             &mut self,
//             _tx: &Outputter<Self::Tx>,
//             _hdr: &crate::WireHeader,
//             body: &[u8],
//         ) -> Result<(), ()> {
//             if (body[0] & 0x1) == 0 {
//                 Ok(())
//             } else {
//                 Err(())
//             }
//         }
//     }

//     async fn hate_odds(buf: &[u8]) -> Result<(), ()> {
//         if (buf[0] & 0x1) == 0 {
//             Ok(())
//         } else {
//             Err(())
//         }
//     }
// }
