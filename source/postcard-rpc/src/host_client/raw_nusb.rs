//! Implementation of transport using nusb

use std::future::Future;

use nusb::{
    transfer::{Queue, RequestBuffer, TransferError},
    DeviceInfo,
};
use postcard_schema::Schema;
use serde::de::DeserializeOwned;

use crate::{
    header::VarSeqKind,
    host_client::{HostClient, WireRx, WireSpawn, WireTx},
};

// TODO: These should all be configurable, PRs welcome

/// The Bulk Out Endpoint (0x00 | 0x01): Out EP 1
pub(crate) const BULK_OUT_EP: u8 = 0x01;
/// The Bulk In Endpoint (0x80 | 0x01): In EP 1
pub(crate) const BULK_IN_EP: u8 = 0x81;
/// The size in bytes of the largest possible IN transfer
pub(crate) const MAX_TRANSFER_SIZE: usize = 1024;
/// How many in-flight requests at once - allows nusb to keep pulling frames
/// even if we haven't processed them host-side yet.
pub(crate) const IN_FLIGHT_REQS: usize = 4;
/// How many consecutive IN errors will we try to recover from before giving up?
pub(crate) const MAX_STALL_RETRIES: usize = 10;

/// # `nusb` Constructor Methods
///
/// These methods are used to create a new [HostClient] instance for use with `nusb` and
/// USB bulk transfer encoding.
///
/// **Requires feature**: `raw-nusb`
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Try to create a new link using [`nusb`] for connectivity
    ///
    /// The provided function will be used to find a matching device. The first
    /// matching device will be connected to. `err_uri_path` is
    /// the path associated with the `WireErr` message type.
    ///
    /// Returns an error if no device could be found, or if there was an error
    /// connecting to the device.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// let client = HostClient::<Error>::try_new_raw_nusb(
    ///     // Find the first device with the serial 12345678
    ///     |d| d.serial_number() == Some("12345678"),
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Use one-byte sequence numbers
    ///     VarSeqKind::Seq1,
    /// ).unwrap();
    /// ```
    pub fn try_new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let x = nusb::list_devices()
            .map_err(|e| format!("Error listing devices: {e:?}"))?
            .find(func)
            .ok_or_else(|| String::from("Failed to find matching nusb device!"))?;
        let dev = x
            .open()
            .map_err(|e| format!("Failed opening device: {e:?}"))?;
        let interface = dev
            .claim_interface(0)
            .map_err(|e| format!("Failed claiming interface: {e:?}"))?;

        let boq = interface.bulk_out_queue(BULK_OUT_EP);
        let biq = interface.bulk_in_queue(BULK_IN_EP);

        Ok(HostClient::new_with_wire(
            NusbWireTx { boq },
            NusbWireRx {
                biq,
                consecutive_errs: 0,
            },
            NusbSpawn,
            seq_no_kind,
            err_uri_path,
            outgoing_depth,
        ))
    }

    /// Create a new link using [`nusb`] for connectivity
    ///
    /// Panics if connection fails. See [`Self::try_new_raw_nusb()`] for more details.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// let client = HostClient::<Error>::new_raw_nusb(
    ///     // Find the first device with the serial 12345678
    ///     |d| d.serial_number() == Some("12345678"),
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Use one-byte sequence numbers
    ///     VarSeqKind::Seq1,
    /// );
    /// ```
    pub fn new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Self {
        Self::try_new_raw_nusb(func, err_uri_path, outgoing_depth, seq_no_kind)
            .expect("should have found nusb device")
    }
}

//////////////////////////////////////////////////////////////////////////////
// Wire Interface Implementation
//////////////////////////////////////////////////////////////////////////////

/// NUSB Wire Interface Implementor
///
/// Uses Tokio for spawning tasks
struct NusbSpawn;

impl WireSpawn for NusbSpawn {
    fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) {
        // Explicitly drop the joinhandle as it impls Future and this makes
        // clippy mad if you just let it drop implicitly
        core::mem::drop(tokio::task::spawn(fut));
    }
}

/// NUSB Wire Transmit Interface Implementor
struct NusbWireTx {
    boq: Queue<Vec<u8>>,
}

#[derive(thiserror::Error, Debug)]
enum NusbWireTxError {
    #[error("Transfer Error on Send")]
    Transfer(#[from] TransferError),
}

impl WireTx for NusbWireTx {
    type Error = NusbWireTxError;

    #[inline]
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.send_inner(data)
    }
}

impl NusbWireTx {
    async fn send_inner(&mut self, data: Vec<u8>) -> Result<(), NusbWireTxError> {
        self.boq.submit(data);

        let send_res = self.boq.next_complete().await;
        if let Err(e) = send_res.status {
            tracing::error!("Output Queue Error: {e:?}");
            return Err(e.into());
        }
        Ok(())
    }
}

/// NUSB Wire Receive Interface Implementor
struct NusbWireRx {
    biq: Queue<RequestBuffer>,
    consecutive_errs: usize,
}

#[derive(thiserror::Error, Debug)]
enum NusbWireRxError {
    #[error("Transfer Error on Recv")]
    Transfer(#[from] TransferError),
}

impl WireRx for NusbWireRx {
    type Error = NusbWireRxError;

    #[inline]
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        self.recv_inner()
    }
}

impl NusbWireRx {
    async fn recv_inner(&mut self) -> Result<Vec<u8>, NusbWireRxError> {
        loop {
            // Rehydrate the queue
            let pending = self.biq.pending();
            for _ in 0..(IN_FLIGHT_REQS.saturating_sub(pending)) {
                self.biq.submit(RequestBuffer::new(MAX_TRANSFER_SIZE));
            }

            let res = self.biq.next_complete().await;

            if let Err(e) = res.status {
                self.consecutive_errs += 1;

                tracing::error!(
                    "In Worker error: {e:?}, consecutive: {}",
                    self.consecutive_errs
                );

                // Docs only recommend this for Stall, but it seems to work with
                // UNKNOWN on MacOS as well, todo: look into why!
                //
                // Update: This stall condition seems to have been due to an errata in the
                // STM32F4 USB hardware. See https://github.com/embassy-rs/embassy/pull/2823
                //
                // It is now questionable whether we should be doing this stall recovery at all,
                // as it likely indicates an issue with the connected USB device
                let recoverable = match e {
                    TransferError::Stall | TransferError::Unknown => {
                        self.consecutive_errs <= MAX_STALL_RETRIES
                    }
                    TransferError::Cancelled => false,
                    TransferError::Disconnected => false,
                    TransferError::Fault => false,
                };

                let fatal = if recoverable {
                    tracing::warn!("Attempting stall recovery!");

                    // Stall recovery shouldn't be used with in-flight requests, so
                    // cancel them all. They'll still pop out of next_complete.
                    self.biq.cancel_all();
                    tracing::info!("Cancelled all in-flight requests");

                    // Now we need to join all in flight requests
                    for _ in 0..(IN_FLIGHT_REQS - 1) {
                        let res = self.biq.next_complete().await;
                        tracing::info!("Drain state: {:?}", res.status);
                    }

                    // Now we can mark the stall as clear
                    match self.biq.clear_halt() {
                        Ok(()) => false,
                        Err(e) => {
                            tracing::error!("Failed to clear stall: {e:?}, Fatal.");
                            true
                        }
                    }
                } else {
                    tracing::error!(
                        "Giving up after {} errors in a row, final error: {e:?}",
                        self.consecutive_errs
                    );
                    true
                };

                if fatal {
                    tracing::error!("Fatal Error, exiting");
                    // TODO we should notify sub worker!
                    // ctxt.map.close();
                    return Err(e.into());
                } else {
                    tracing::info!("Potential recovery, resuming NusbWireRx::recv_inner");
                    continue;
                }
            }

            // TODO: Min size of a header is 9 bytes, 8 for key, 1 for seq_no.
            if res.data.len() < 9 {
                tracing::warn!("Header decode error!");
                continue;
            };

            // If we get a good decode, clear the error flag
            if self.consecutive_errs != 0 {
                tracing::info!("Clearing consecutive error counter after good header decode");
                self.consecutive_errs = 0;
            }

            return Ok(res.data);
        }
    }
}
