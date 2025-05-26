//! Implementation using `embassy-net` TCP sockets.
use core::fmt::Arguments;

use embassy_net::tcp::{self, TcpReader, TcpWriter};
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};

use crate::server::{WireRx, WireRxErrorKind, WireTx, WireTxErrorKind};

/// A collection of types and aliases
pub mod dispatch_impl {
    use embassy_net::tcp::TcpSocket;
    use embassy_net::IpListenEndpoint;
    use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
    use static_cell::StaticCell;

    pub use crate::server::impls::embassy_usb_v0_4::embassy_spawn as spawn_fn;
    use crate::server::impls::embassy_usb_v0_4::EUsbWireSpawn;
    pub use crate::server::impls::embassy_usb_v0_4::PacketBuffers;

    use super::ENetTcpWireTxInner;

    /// Type alias for `WireTx` imple
    pub type WireTxImpl<M> = super::ENetTcpWireTx<M>;
    /// Type alias for `WireRx` imple
    pub type WireRxImpl = super::ENetTcpWireRx;
    /// Re-use the WireSpawn impl from the embassy-usb implementation
    pub type WireSpawnImpl = EUsbWireSpawn;
    /// Type alias for the receive buffer
    pub type WireRxBuf = &'static mut [u8];

    /// Static storage for the server
    pub struct WireStorage<M: RawMutex + 'static> {
        /// WireTx static storage
        pub cell: StaticCell<Mutex<M, ENetTcpWireTxInner<'static>>>,
    }

    impl<M: RawMutex + 'static> WireStorage<M> {
        /// Create a new uninitialised storage
        pub const fn new() -> Self {
            Self {
                cell: StaticCell::new(),
            }
        }

        /// Initialise the server and storage from a socket
        pub async fn accept<E: Into<IpListenEndpoint>>(
            &'static self,
            sock: &'static mut TcpSocket<'static>,
            e: E,
            tx_buf: &'static mut [u8],
        ) -> (WireTxImpl<M>, WireRxImpl) {
            sock.accept(e).await.unwrap();
            let (reader, writer) = sock.split();
            let wtx = self
                .cell
                .init(Mutex::new(ENetTcpWireTxInner { writer, tx_buf }));
            (WireTxImpl { inner: wtx }, WireRxImpl { reader })
        }
    }
}

/// Implementation detail - holds the TCP writer and buffer
pub struct ENetTcpWireTxInner<'a> {
    writer: TcpWriter<'a>,
    tx_buf: &'static mut [u8],
}

/// A [`WireTx`] implementation for embassy-net TCP
pub struct ENetTcpWireTx<M: RawMutex + 'static> {
    inner: &'static Mutex<M, ENetTcpWireTxInner<'static>>,
}

impl<M: RawMutex + 'static> WireTx for ENetTcpWireTx<M> {
    type Error = WireTxErrorKind;

    async fn send<T: serde::Serialize + ?Sized>(
        &self,
        hdr: crate::header::VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;

        let ENetTcpWireTxInner { writer, tx_buf }: &mut ENetTcpWireTxInner = &mut inner;

        let (hdr_used, remain) = hdr.write_to_slice(tx_buf).ok_or(WireTxErrorKind::Other)?;
        let bdy_used = postcard::to_slice(msg, remain).map_err(|_| WireTxErrorKind::Other)?;
        let used_ttl = hdr_used.len() + bdy_used.len();
        if let Some(used) = tx_buf.get(..used_ttl) {
            let written = writer
                .write(used)
                .await
                .map_err(|_| WireTxErrorKind::Other)?;
            if written != used_ttl {
                Err(WireTxErrorKind::Other)
            } else {
                Ok(())
            }
        } else {
            Err(WireTxErrorKind::Other)
        }
    }

    async fn wait_connection(&self) {
        let mut inner = self.inner.lock().await;

        let ENetTcpWireTxInner { writer, tx_buf: _ }: &mut ENetTcpWireTxInner = &mut inner;
        writer.wait_write_ready().await;
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;

        let ENetTcpWireTxInner { writer, tx_buf: _ }: &mut ENetTcpWireTxInner = &mut inner;
        let written = writer
            .write(buf)
            .await
            .map_err(|_| WireTxErrorKind::Other)?;
        if written != buf.len() {
            Err(WireTxErrorKind::Other)
        } else {
            Ok(())
        }
    }

    async fn send_log_str(
        &self,
        kkind: crate::header::VarKeyKind,
        s: &str,
    ) -> Result<(), Self::Error> {
        todo!()
    }

    async fn send_log_fmt<'a>(
        &self,
        kind: crate::header::VarKeyKind,
        a: Arguments<'a>,
    ) -> Result<(), Self::Error> {
        todo!()
    }
}

/// A [`WireRx`] implementation for embassy-net TCP
pub struct ENetTcpWireRx {
    reader: TcpReader<'static>,
}

impl WireRx for ENetTcpWireRx {
    type Error = WireRxErrorKind;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        let buflen = buf.len();
        let n = match self.reader.read(buf).await {
            Ok(n) => n,
            Err(tcp::Error::ConnectionReset) => return Err(WireRxErrorKind::ConnectionClosed),
        };

        if n == buflen {
            // out of space?
            unimplemented!()
        }

        Ok(&mut buf[..n])
    }

    async fn wait_connection(&mut self) {
        self.reader.wait_read_ready().await
    }
}
