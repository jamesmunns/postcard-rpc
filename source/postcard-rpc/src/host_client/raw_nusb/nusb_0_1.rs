use std::future::Future;

use nusb_0_1::{
    self as nusb,
    descriptors::Endpoint,
    transfer::{Queue, RequestBuffer, TransferError},
};

pub use nusb::*;

use crate::host_client::{WireRx, WireTx};

// TODO: This should be configurable, PRs welcome
/// How many consecutive IN errors will we try to recover from before giving up?
pub(crate) const MAX_STALL_RETRIES: usize = 10;

//////////////////////////////////////////////////////////////////////////////
// Wrappers for common functionality that is slightly different from nusb 0.1 <-> 0.2
//////////////////////////////////////////////////////////////////////////////

pub(crate) fn open_device(dev: &nusb::DeviceInfo) -> Result<Device, nusb::Error> {
    dev.open()
}

pub(crate) fn claim_interface(dev: &Device, interface: u8) -> Result<Interface, nusb::Error> {
    dev.claim_interface(interface)
}

pub(crate) fn is_bulk_endpoint(e: &Endpoint) -> bool {
    e.transfer_type() == nusb::transfer::EndpointType::Bulk
}

pub(crate) fn make_tx_impl(
    interface: &Interface,
    ep_out: u8,
    max_packet_size: Option<usize>,
) -> Result<impl WireTx, String> {
    let boq = interface.bulk_out_queue(ep_out);

    Ok(NusbWireTx {
        boq,
        max_packet_size,
    })
}

pub(crate) fn make_rx_impl(interface: &Interface, ep_in: u8) -> Result<impl WireRx, String> {
    let biq = interface.bulk_in_queue(ep_in);

    Ok(NusbWireRx {
        biq,
        consecutive_errs: 0,
    })
}

//////////////////////////////////////////////////////////////////////////////
// Wire Interface Implementation
//////////////////////////////////////////////////////////////////////////////

/// NUSB 0.1 Wire Transmit Interface Implementor
pub(crate) struct NusbWireTx {
    pub boq: Queue<Vec<u8>>,
    pub max_packet_size: Option<usize>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum NusbWireTxError {
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
        let needs_zlp = if let Some(mps) = self.max_packet_size {
            (data.len() % mps) == 0
        } else {
            true
        };

        self.boq.submit(data);

        // Append ZLP if we are a multiple of max packet
        if needs_zlp {
            self.boq.submit(vec![]);
        }

        let send_res = self.boq.next_complete().await;
        if let Err(e) = send_res.status {
            tracing::error!("Output Queue Error: {e:?}");
            return Err(e.into());
        }

        if needs_zlp {
            let send_res = self.boq.next_complete().await;
            if let Err(e) = send_res.status {
                tracing::error!("Output Queue Error: {e:?}");
                return Err(e.into());
            }
        }

        Ok(())
    }
}

/// NUSB 0.1 Wire Receive Interface Implementor
pub(crate) struct NusbWireRx {
    pub biq: Queue<RequestBuffer>,
    pub consecutive_errs: usize,
}

#[derive(thiserror::Error, Debug)]
pub enum NusbWireRxError {
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
        use super::{IN_FLIGHT_REQS, MAX_TRANSFER_SIZE};

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
                    // When we close the channel, all pending receivers and subscribers
                    // will be notified
                    return Err(e.into());
                } else {
                    tracing::info!("Potential recovery, resuming NusbWireRx::recv_inner");
                    continue;
                }
            }

            // If we get a good decode, clear the error flag
            if self.consecutive_errs != 0 {
                tracing::info!("Clearing consecutive error counter after good header decode");
                self.consecutive_errs = 0;
            }

            return Ok(res.data);
        }
    }
}
