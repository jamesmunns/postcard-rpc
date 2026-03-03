use std::future::Future;

use nusb_0_2::{
    self as nusb,
    descriptors::{EndpointDescriptor, TransferType},
    io::{EndpointRead, EndpointWrite},
    transfer::{Bulk, In, Out, TransferError},
};

pub use nusb::*;

use crate::host_client::{WireRx, WireTx};

//////////////////////////////////////////////////////////////////////////////
// Wrappers for common functionality that is slightly different from nusb 0.1 <-> 0.2
//////////////////////////////////////////////////////////////////////////////

/// Blocking wrapper for `nusb_0_2::list_devices`
#[cfg(not(target_family = "wasm"))]
pub fn list_devices() -> Result<impl Iterator<Item = DeviceInfo>, nusb::Error> {
    use nusb_0_2::MaybeFuture;
    nusb::list_devices().wait()
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn open_device(dev: &DeviceInfo) -> Result<Device, nusb::Error> {
    use nusb_0_2::MaybeFuture;
    dev.open().wait()
}

#[cfg(not(target_family = "wasm"))]
pub(crate) fn claim_interface(dev: &Device, interface: u8) -> Result<Interface, nusb::Error> {
    use nusb_0_2::MaybeFuture;
    dev.claim_interface(interface).wait()
}

pub(crate) fn is_bulk_endpoint(e: &EndpointDescriptor) -> bool {
    e.transfer_type() == TransferType::Bulk
}

pub(crate) fn make_tx_impl(
    interface: &Interface,
    ep_out: u8,
    _max_packet_size: Option<usize>,
) -> Result<impl WireTx, String> {
    let writer = interface
        .endpoint::<Bulk, Out>(ep_out)
        .map_err(|e| format!("Failed to claim OUT endpoint: {e:?}"))?
        .writer(super::MAX_TRANSFER_SIZE)
        .with_num_transfers(super::IN_FLIGHT_REQS);

    Ok(NusbWireTx { writer })
}

pub(crate) fn make_rx_impl(interface: &Interface, ep_in: u8) -> Result<impl WireRx, String> {
    let reader = interface
        .endpoint::<Bulk, In>(ep_in)
        .map_err(|e| format!("Failed to claim IN endpoint: {e:?}"))?
        .reader(super::MAX_TRANSFER_SIZE)
        .with_num_transfers(super::IN_FLIGHT_REQS);

    Ok(NusbWireRx { reader })
}

//////////////////////////////////////////////////////////////////////////////
// Wire Interface Implementation
//////////////////////////////////////////////////////////////////////////////

/// NUSB 0.2 Wire Transmit Interface Implementor
pub(crate) struct NusbWireTx {
    pub writer: EndpointWrite<Bulk>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum NusbWireTxError {
    #[error("Transfer Error on Send")]
    Transfer(#[from] TransferError),
    #[error("I/O Error on Send")]
    Io(#[from] std::io::Error),
}

impl WireTx for NusbWireTx {
    type Error = NusbWireTxError;

    #[inline]
    #[cfg(not(target_family = "wasm"))]
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.send_inner(data)
    }

    #[inline]
    #[cfg(target_family = "wasm")]
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> {
        self.send_inner(data)
    }
}

impl NusbWireTx {
    async fn send_inner(&mut self, data: Vec<u8>) -> Result<(), NusbWireTxError> {
        #[cfg(feature = "tokio")]
        use tokio::io::AsyncWriteExt;

        #[cfg(all(feature = "futures-lite", not(feature = "tokio")))]
        use futures_lite::io::AsyncWriteExt;

        self.writer.write_all(&data).await?;
        self.writer.flush_end_async().await?;

        Ok(())
    }
}

/// NUSB 0.2 Wire Receive Interface Implementor
pub(crate) struct NusbWireRx {
    pub reader: EndpointRead<transfer::Bulk>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum NusbWireRxError {
    #[error("Transfer Error on Recv")]
    Transfer(#[from] transfer::TransferError),
    #[error("I/O Error on Recv")]
    IO(#[from] std::io::Error),
    #[error("Short Packet Error From nusb")]
    ExpectedShortPacket(#[from] nusb_0_2::io::ExpectedShortPacket),
}

impl WireRx for NusbWireRx {
    type Error = NusbWireRxError;

    #[inline]
    #[cfg(not(target_family = "wasm"))]
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        self.recv_inner()
    }

    #[inline]
    #[cfg(target_family = "wasm")]
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> {
        self.recv_inner()
    }
}

impl NusbWireRx {
    async fn recv_inner(&mut self) -> Result<Vec<u8>, NusbWireRxError> {
        #[cfg(feature = "tokio")]
        use tokio::io::AsyncReadExt;

        #[cfg(all(feature = "futures-lite", not(feature = "tokio")))]
        use futures_lite::io::AsyncReadExt;

        let mut reader = self.reader.until_short_packet();
        let mut v = Vec::new();

        reader.read_to_end(&mut v).await?;
        reader.consume_end()?;

        Ok(v)
    }
}
