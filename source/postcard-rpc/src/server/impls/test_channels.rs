//! Implementation that uses channels for local testing

use core::{convert::Infallible, future::Future};

use crate::server::{
    AsWireRxErrorKind, AsWireTxErrorKind, WireRx, WireRxErrorKind, WireSpawn, WireTx,
    WireTxErrorKind,
};
use tokio::sync::mpsc;

//////////////////////////////////////////////////////////////////////////////
// DISPATCH IMPL
//////////////////////////////////////////////////////////////////////////////

pub mod dispatch_impl {
    pub struct Settings {
        pub tx: WireTxImpl,
        pub rx: WireRxImpl,
        pub buf: usize,
        pub kkind: VarKeyKind,
    }

    pub type WireTxImpl = super::ChannelWireTx;
    pub type WireRxImpl = super::ChannelWireRx;
    pub type WireSpawnImpl = super::ChannelWireSpawn;
    pub type WireRxBuf = Box<[u8]>;

    use crate::{
        header::VarKeyKind,
        server::{Dispatch, Server},
    };

    pub use super::tokio_spawn as spawn_fn;

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

#[derive(Clone)]
pub struct ChannelWireTx {
    tx: mpsc::Sender<Vec<u8>>,
}

impl ChannelWireTx {
    pub fn new(tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            tx
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
}

#[derive(Debug)]
pub enum ChannelWireTxError {
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

pub struct ChannelWireRx {
    rx: mpsc::Receiver<Vec<u8>>,
}

impl ChannelWireRx {
    pub fn new(rx: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx
        }
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

#[derive(Debug)]
pub enum ChannelWireRxError {
    ChannelClosed,
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

// todo: just use a standard tokio impl?
#[derive(Clone)]
pub struct ChannelWireSpawn {}

impl WireSpawn for ChannelWireSpawn {
    type Error = Infallible;

    type Info = ();

    fn info(&self) -> &Self::Info {
        &()
    }
}

pub fn tokio_spawn<Sp, F>(_sp: &Sp, fut: F) -> Result<(), Sp::Error>
where
    Sp: WireSpawn<Error = Infallible, Info = ()>,
    F: Future<Output = ()> + 'static + Send,
{
    tokio::task::spawn(fut);
    Ok(())
}
