//! TCP client
use std::fmt::{Debug, Display};
use std::error::Error;
use std::net::SocketAddr;
use std::future::Future;

use postcard_schema::Schema;
use serde::de::DeserializeOwned;
use tokio::io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

use crate::header;
use crate::standard_icd::ERROR_PATH;

use super::{HostClient, WireRx, WireSpawn, WireTx};

/// Error during TCP RX
pub enum TcpCommsRxError {
    /// Rx buffer overflow
    RxOverflow,
    /// General connection error
    ConnError,
}

impl Debug for TcpCommsRxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("oops")
    }
}

impl Display for TcpCommsRxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("oops")
    }
}

impl Error for TcpCommsRxError {}

struct TcpCommsRx<T: AsyncRead + Send + 'static> {
    addr: SocketAddr,
    buf: Vec<u8>,
    rx: ReadHalf<T>,
}

impl<T: AsyncRead + Send + 'static> TcpCommsRx<T> {
    async fn receive_inner(&mut self) -> Result<Vec<u8>, TcpCommsRxError> {
        let mut rx_buf = [0u8; 1024];
        'frame: loop {
            if self.buf.len() > (1024 * 1024) {
                tracing::warn!(?self.addr, "Refusing to collect >1MiB, terminating");
                self.buf.clear();
                return Err(TcpCommsRxError::RxOverflow);
            }

            // Do we have a message already?
            if let Some(pos) = self.buf.iter().position(|b| *b == 0) {
                // we found the end of a message, attempt to decode it
                let mut split = self.buf.split_off(pos + 1);
                core::mem::swap(&mut self.buf, &mut split);

                // Can we decode the cobs?
                let res = cobs::decode_vec(&split);
                let Ok(msg) = res else {
                    tracing::warn!(?self.addr, discarded = split.len(), "Discarding bad message (cobs)");
                    continue 'frame;
                };

                return Ok(msg);
            }

            // No message yet, let's try and receive some data
            let Ok(used) = self.rx.read(&mut rx_buf).await else {
                tracing::warn!(?self.addr, "Closing");
                return Err(TcpCommsRxError::ConnError);
            };
            if used == 0 {
                tracing::warn!(?self.addr, "Closing");
                return Err(TcpCommsRxError::ConnError);
            }
            self.buf.extend_from_slice(&rx_buf[..used]);
        }
    }
}

impl<T: AsyncRead + Send + 'static> WireRx for TcpCommsRx<T> {
    type Error = TcpCommsRxError;

    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        self.receive_inner()
    }
}

/// An error during TX
pub enum TcpCommsTxError {
    /// A general tx comms error
    CommsError,
}

impl Debug for TcpCommsTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("oops")
    }
}

impl Display for TcpCommsTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("oops")
    }
}

impl Error for TcpCommsTxError {}

struct TcpCommsTx<T: AsyncWrite + Send + 'static> {
    tx: WriteHalf<T>,
}

impl<T: AsyncWrite + Send + 'static> TcpCommsTx<T> {
    async fn send_inner(&mut self, data: Vec<u8>) -> Result<(), TcpCommsTxError> {
        let mut data = cobs::encode_vec(&data);
        data.push(0);
        self.tx
            .write_all(&data)
            .await
            .map_err(|_| TcpCommsTxError::CommsError)
    }
}

impl<T: AsyncWrite + Send + 'static> WireTx for TcpCommsTx<T> {
    type Error = TcpCommsTxError;

    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.send_inner(data)
    }
}

// ---

struct TcpSpawn;

impl WireSpawn for TcpSpawn {
    fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) {
        tokio::spawn(fut);
    }
}

impl <WireErr> HostClient<WireErr> where WireErr: DeserializeOwned + Schema {
    /// Connect to a server via TCP
    pub async fn connect_tcp<T>(addr: T) -> Self where T: tokio::net::ToSocketAddrs {
        let stream = TcpStream::connect(addr).await.unwrap();
        let addr = stream.peer_addr().unwrap();
        let (rx, tx) = split(stream);
        HostClient::new_with_wire(
            TcpCommsTx {
                tx 
            },
            TcpCommsRx {
                rx,
                addr,
                buf: vec![],
            },
            TcpSpawn,
            header::VarSeqKind::Seq4,
            ERROR_PATH,
            64,
        )
    }

}
