#![allow(async_fn_in_trait)]

pub mod dispatch_macro;

pub mod impls;

use core::ops::DerefMut;

use postcard_schema::Schema;
use serde::Serialize;

use crate::{
    header::{VarHeader, VarKey, VarKeyKind, VarSeq},
    Key,
};

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

pub trait WireTx: Clone {
    type Error: AsWireTxErrorKind;
    async fn send<T: Serialize + ?Sized>(&self, hdr: VarHeader, msg: &T)
        -> Result<(), Self::Error>;
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
// SENDER (wrapper of WireTx)
//////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct Sender<Tx: WireTx> {
    tx: Tx,
    kkind: VarKeyKind,
}

impl<Tx: WireTx> Sender<Tx> {
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
        T: crate::Topic,
        T::Message: Serialize + Schema,
    {
        let mut key = VarKey::Key8(T::TOPIC_KEY);
        key.shrink_to(self.kkind);
        let wh = VarHeader { key, seq_no };
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
    D: Dispatch<Tx = Tx>,
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
    D: Dispatch<Tx = Tx>,
{
    pub fn new(tx: &Tx, rx: Rx, buf: Buf, dis: D, kkind: VarKeyKind) -> Self {
        Self {
            tx: Sender {
                tx: tx.clone(),
                kkind,
            },
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
                }
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// DISPATCH TRAIT
//////////////////////////////////////////////////////////////////////////////

pub trait Dispatch {
    type Tx: WireTx;
    fn min_key_len(&self) -> VarKeyKind;

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
        u64::from_le_bytes(key.0)
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
