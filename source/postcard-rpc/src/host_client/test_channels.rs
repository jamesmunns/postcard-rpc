use core::fmt::Display;

use tokio::sync::mpsc;

use crate::standard_icd::WireError;

use super::{HostClient, WireRx, WireSpawn, WireTx};

pub fn new_from_channels(tx: mpsc::Sender<Vec<u8>>, rx: mpsc::Receiver<Vec<u8>>) -> HostClient<WireError> {
    HostClient::new_with_wire(
        ChannelTx { tx },
        ChannelRx { rx },
        TokSpawn,
        crate::standard_icd::ERROR_PATH,
        64,
    )
}

#[derive(Debug)]
pub enum ChannelError {
    RxClosed,
    TxClosed,
}

impl Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as core::fmt::Debug>::fmt(self, f)
    }
}

impl std::error::Error for ChannelError {}

pub struct ChannelRx {
    rx: mpsc::Receiver<Vec<u8>>,
}
pub struct ChannelTx {
    tx: mpsc::Sender<Vec<u8>>,
}
pub struct ChannelSpawn;


impl WireRx for ChannelRx {
    type Error = ChannelError;

    async fn receive(&mut self) -> Result<Vec<u8>, Self::Error> {
        match self.rx.recv().await {
            Some(v) => Ok(v),
            None => Err(ChannelError::RxClosed),
        }
    }
}


impl WireTx for ChannelTx {
    type Error = ChannelError;

    async fn send(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
        self.tx.send(data).await.map_err(|_| ChannelError::TxClosed)
    }
}

struct TokSpawn;
impl WireSpawn for TokSpawn {
    fn spawn(&mut self, fut: impl std::future::Future<Output = ()> + Send + 'static) {
        _ = tokio::task::spawn(fut);
    }
}
