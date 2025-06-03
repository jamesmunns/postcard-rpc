//! Implementation using `embassy-net` TCP sockets.
use core::fmt::Arguments;

use embassy_net::{
    tcp::{self, State, TcpReader, TcpSocket, TcpWriter},
    IpListenEndpoint,
};
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use postcard::to_vec_cobs;

use crate::server::{WireRx, WireRxErrorKind, WireTx, WireTxErrorKind};

/// A collection of types and aliases
pub mod dispatch_impl {
    use embassy_time::Duration;

    use embassy_net::tcp::TcpSocket;
    use embassy_net::IpListenEndpoint;
    use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
    use static_cell::StaticCell;

    pub use crate::server::impls::embassy_usb_v0_4::embassy_spawn as spawn_fn;
    use crate::server::impls::embassy_usb_v0_4::EUsbWireSpawn;
    pub use crate::server::impls::embassy_usb_v0_4::PacketBuffers;

    use super::{ENetTcpWireRx, ENetTcpWireTx, ENetTcpWireTxRxInner};

    /// Type alias for `WireTx` imple
    pub type WireTxImpl<M> = super::ENetTcpWireTx<M>;
    /// Type alias for `WireRx` imple
    pub type WireRxImpl<M> = super::ENetTcpWireRx<M>;
    /// Re-use the WireSpawn impl from the embassy-usb implementation
    pub type WireSpawnImpl = EUsbWireSpawn;
    /// Type alias for the receive buffer
    pub type WireRxBuf = &'static mut [u8];

    /// Static storage for the server
    pub struct WireStorage<M: RawMutex + 'static> {
        /// WireTx static storage
        pub cell: StaticCell<Mutex<M, ENetTcpWireTxRxInner<'static>>>,
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
            mut sock: TcpSocket<'static>,
            endpoint: E,
            tx_buf: &'static mut [u8],
        ) -> (WireTxImpl<M>, WireRxImpl<M>) {
            sock.set_keep_alive(Some(Duration::from_secs(2)));
            sock.set_timeout(Some(Duration::from_secs(3)));
            let inner = self
                .cell
                .init(Mutex::new(ENetTcpWireTxRxInner { sock, tx_buf }));
            (
                WireTxImpl { inner },
                WireRxImpl {
                    inner,
                    endpoint: endpoint.into(),
                },
            )
        }
    }
}

/// Implementation detail - holds the TCP writer and buffer
pub struct ENetTcpWireTxRxInner<'a> {
    sock: TcpSocket<'a>,
    tx_buf: &'a mut [u8],
}

/// A [`WireTx`] implementation for embassy-net TCP
pub struct ENetTcpWireTx<M: RawMutex + 'static> {
    inner: &'static Mutex<M, ENetTcpWireTxRxInner<'static>>,
}

/// A [`WireTx`] and [`WireRx`] implementation wrapping a tcp socket
pub struct ENetTcpWireRx<M: RawMutex + 'static> {
    inner: &'static Mutex<M, ENetTcpWireTxRxInner<'static>>,
    endpoint: IpListenEndpoint,
}

impl<M: RawMutex + 'static> WireTx for ENetTcpWireTx<M> {
    type Error = WireTxErrorKind;

    async fn send<T: serde::Serialize + ?Sized>(
        &self,
        hdr: crate::header::VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;

        let ENetTcpWireTxRxInner { sock, tx_buf }: &mut ENetTcpWireTxRxInner = &mut inner;

        let (hdr_used, remain) = hdr.write_to_slice(tx_buf).ok_or(WireTxErrorKind::Other)?;
        let bdy_used = postcard::to_slice(msg, remain).map_err(|_| WireTxErrorKind::Other)?;
        let used_ttl = hdr_used.len() + bdy_used.len();
        let (raw, rest) = tx_buf.split_at_mut(used_ttl);
        if cobs::max_encoding_length(used_ttl) > rest.len() {
            // not enough space to squeeze the cobs encoded data in the rest of the tx buf.
            // TODO: this is a hack, do better buffer management.
            return Err(WireTxErrorKind::Other);
        }
        let encobsed = cobs::encode(raw, rest);
        rest[encobsed] = 0;
        let written = sock
            .write(&rest[..=encobsed])
            .await
            .map_err(|_| WireTxErrorKind::Other)?;
        #[cfg(feature = "defmt")]
        defmt::info!("wrote {} rpc bytes", written);
        if written != used_ttl {
            Err(WireTxErrorKind::Other)
        } else {
            Ok(())
        }
    }

    async fn wait_connection(&self) {}

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        #[cfg(feature = "defmt")]
        defmt::info!("tx send_raw: waiting for lock");
        let mut inner = self.inner.lock().await;

        let ENetTcpWireTxRxInner { sock, tx_buf: _ }: &mut ENetTcpWireTxRxInner = &mut inner;
        #[cfg(feature = "defmt")]
        defmt::debug!("waiting for tx ready");
        sock.wait_write_ready().await;
        #[cfg(feature = "defmt")]
        defmt::debug!("writing {} bytes", buf.len());
        let written = sock.write(buf).await.map_err(|_| WireTxErrorKind::Other)?;
        #[cfg(feature = "defmt")]
        defmt::debug!("wrote {} bytes", written);
        sock.flush().await.map_err(|_| WireTxErrorKind::Other)?;
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

impl<M: RawMutex + 'static> WireRx for ENetTcpWireRx<M> {
    type Error = WireRxErrorKind;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        #[cfg(feature = "defmt")]
        defmt::debug!("receive: wait lock");
        let buflen = buf.len();
        let mut inner = self.inner.lock().await;
        let ENetTcpWireTxRxInner { sock, tx_buf: _ }: &mut ENetTcpWireTxRxInner = &mut inner;
        #[cfg(feature = "defmt")]
        defmt::debug!("receive: read");
        let n = match sock.read(buf).await {
            Ok(0) | Err(tcp::Error::ConnectionReset) => {
                return Err(WireRxErrorKind::ConnectionClosed);
            }
            Ok(n) => n,
        };
        #[cfg(feature = "defmt")]
        defmt::debug!("received {} rpc bytes", n);

        if n == buflen {
            // out of space?
            unimplemented!()
        }

        Ok(&mut buf[..n])
    }

    async fn wait_connection(&mut self) {
        #[cfg(feature = "defmt")]
        defmt::debug!("rx wait for lock");
        let mut inner = self.inner.lock().await;
        let ENetTcpWireTxRxInner { sock, tx_buf: _ }: &mut ENetTcpWireTxRxInner = &mut inner;
        #[cfg(feature = "defmt")]
        defmt::debug!("rx sock state {}", sock.state());
        match sock.state() {
            State::Established | State::CloseWait | State::Closing => return,
            _ => {
                #[cfg(feature = "defmt")]
                defmt::info!("Waiting for RPC connection");
                sock.accept(self.endpoint).await.unwrap();
                #[cfg(feature = "defmt")]
                defmt::info!("remote connected: {}", sock.remote_endpoint());
            }
        }
    }
}
