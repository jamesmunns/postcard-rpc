//! Implementation that uses channels for local testing

use core::{
    convert::Infallible,
    future::Future,
    sync::atomic::{AtomicU32, Ordering},
};
use std::sync::Arc;

use crate::{
    header::{VarHeader, VarKey, VarKeyKind, VarSeq},
    server::{
        AsWireRxErrorKind, AsWireTxErrorKind, WireRx, WireRxErrorKind, WireSpawn, WireTx,
        WireTxErrorKind,
    },
    standard_icd::LoggingTopic,
    Topic,
};
use core::fmt::Arguments;
use tokio::sync::mpsc;

//////////////////////////////////////////////////////////////////////////////
// DISPATCH IMPL
//////////////////////////////////////////////////////////////////////////////

/// A collection of types and aliases useful for importing the correct types
pub mod dispatch_impl {
    use crate::{
        header::VarKeyKind,
        server::{Dispatch, Server},
    };

    pub use super::tokio_spawn as spawn_fn;

    /// The settings necessary for creating a new channel server
    pub struct Settings {
        /// The frame sender
        pub tx: WireTxImpl,
        /// The frame receiver
        pub rx: WireRxImpl,
        /// The size of the receive buffer
        pub buf: usize,
        /// The sender key size to use
        pub kkind: VarKeyKind,
    }

    /// Type alias for `WireTx` impl
    pub type WireTxImpl = super::ChannelWireTx;
    /// Type alias for `WireRx` impl
    pub type WireRxImpl = super::ChannelWireRx;
    /// Type alias for `WireSpawn` impl
    pub type WireSpawnImpl = super::ChannelWireSpawn;
    /// Type alias for the receive buffer
    pub type WireRxBuf = Box<[u8]>;

    /// Create a new server using the [`Settings`] and [`Dispatch`] implementation
    pub fn new_server<D>(
        dispatch: D,
        settings: Settings,
    ) -> crate::server::Server<WireTxImpl, WireRxImpl, WireRxBuf, D>
    where
        D: Dispatch<Tx = WireTxImpl>,
    {
        let buf = vec![0; settings.buf];
        Server::new(
            &settings.tx,
            settings.rx,
            buf.into_boxed_slice(),
            dispatch,
            settings.kkind,
        )
    }
}

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

/// A [`WireTx`] impl using tokio mpsc channels
#[derive(Clone)]
pub struct ChannelWireTx {
    tx: mpsc::Sender<Vec<u8>>,
    log_ctr: Arc<AtomicU32>,
}

impl ChannelWireTx {
    /// Create a new [`ChannelWireTx`]
    pub fn new(tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            tx,
            log_ctr: Arc::new(AtomicU32::new(0)),
        }
    }
}

impl WireTx for ChannelWireTx {
    type Error = ChannelWireTxError;

    async fn send<T: serde::Serialize + ?Sized>(
        &self,
        hdr: crate::header::VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut hdr_ser = hdr.write_to_vec();
        let bdy_ser = postcard::to_stdvec(msg).unwrap();
        hdr_ser.extend_from_slice(&bdy_ser);
        self.tx
            .send(hdr_ser)
            .await
            .map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let buf = buf.to_vec();
        self.tx
            .send(buf)
            .await
            .map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
    }

    async fn send_log_str(&self, kkind: VarKeyKind, s: &str) -> Result<(), Self::Error> {
        let ctr = self.log_ctr.fetch_add(1, Ordering::Relaxed);
        let key = match kkind {
            VarKeyKind::Key1 => VarKey::Key1(LoggingTopic::TOPIC_KEY1),
            VarKeyKind::Key2 => VarKey::Key2(LoggingTopic::TOPIC_KEY2),
            VarKeyKind::Key4 => VarKey::Key4(LoggingTopic::TOPIC_KEY4),
            VarKeyKind::Key8 => VarKey::Key8(LoggingTopic::TOPIC_KEY),
        };
        let wh = VarHeader {
            key,
            seq_no: VarSeq::Seq4(ctr),
        };
        let msg = s.to_string();

        self.send::<<LoggingTopic as Topic>::Message>(wh, &msg)
            .await
    }

    async fn send_log_fmt<'a>(
        &self,
        kkind: VarKeyKind,
        a: Arguments<'a>,
    ) -> Result<(), Self::Error> {
        let ctr = self.log_ctr.fetch_add(1, Ordering::Relaxed);
        let key = match kkind {
            VarKeyKind::Key1 => VarKey::Key1(LoggingTopic::TOPIC_KEY1),
            VarKeyKind::Key2 => VarKey::Key2(LoggingTopic::TOPIC_KEY2),
            VarKeyKind::Key4 => VarKey::Key4(LoggingTopic::TOPIC_KEY4),
            VarKeyKind::Key8 => VarKey::Key8(LoggingTopic::TOPIC_KEY),
        };
        let wh = VarHeader {
            key,
            seq_no: VarSeq::Seq4(ctr),
        };
        let mut buf = wh.write_to_vec();
        let msg = format!("{a}");
        let msg = postcard::to_stdvec(&msg).unwrap();
        buf.extend_from_slice(&msg);
        self.tx
            .send(buf)
            .await
            .map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
    }
}

/// A wire tx error
#[derive(Debug)]
pub enum ChannelWireTxError {
    /// The receiver closed the channel
    ChannelClosed,
}

impl AsWireTxErrorKind for ChannelWireTxError {
    fn as_kind(&self) -> WireTxErrorKind {
        match self {
            ChannelWireTxError::ChannelClosed => WireTxErrorKind::ConnectionClosed,
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

/// A [`WireRx`] impl using tokio mpsc channels
pub struct ChannelWireRx {
    rx: mpsc::Receiver<Vec<u8>>,
}

impl ChannelWireRx {
    /// Create a new [`ChannelWireRx`]
    pub fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self { rx }
    }
}

impl WireRx for ChannelWireRx {
    type Error = ChannelWireRxError;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        // todo: some kind of receive_owned?
        let msg = self.rx.recv().await;
        let msg = msg.ok_or(ChannelWireRxError::ChannelClosed)?;
        let out = buf
            .get_mut(..msg.len())
            .ok_or(ChannelWireRxError::MessageTooLarge)?;
        out.copy_from_slice(&msg);
        Ok(out)
    }
}

/// A wire rx error
#[derive(Debug)]
pub enum ChannelWireRxError {
    /// The sender closed the channel
    ChannelClosed,
    /// The sender sent a too-large message
    MessageTooLarge,
}

impl AsWireRxErrorKind for ChannelWireRxError {
    fn as_kind(&self) -> WireRxErrorKind {
        match self {
            ChannelWireRxError::ChannelClosed => WireRxErrorKind::ConnectionClosed,
            ChannelWireRxError::MessageTooLarge => WireRxErrorKind::ReceivedMessageTooLarge,
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// SPAWN
//////////////////////////////////////////////////////////////////////////////

/// A wire spawn implementation
#[derive(Clone)]
pub struct ChannelWireSpawn;

impl WireSpawn for ChannelWireSpawn {
    type Error = Infallible;

    type Info = ();

    fn info(&self) -> &Self::Info {
        &()
    }
}

/// Spawn a task using tokio
pub fn tokio_spawn<Sp, F>(_sp: &Sp, fut: F) -> Result<(), Sp::Error>
where
    Sp: WireSpawn<Error = Infallible, Info = ()>,
    F: Future<Output = ()> + 'static + Send,
{
    tokio::task::spawn(fut);
    Ok(())
}
