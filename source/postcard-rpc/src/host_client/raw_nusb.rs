use std::{collections::HashMap, sync::Arc};

use nusb::{
    transfer::{Queue, RequestBuffer},
    DeviceInfo,
};
use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio::sync::{
    mpsc::{error::TrySendError, Receiver, Sender},
    Mutex,
};

use crate::{
    headered::extract_header_from_bytes,
    host_client::{HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext},
    Key,
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

struct NusbCtx {
    sub_map: Mutex<HashMap<Key, Sender<RpcFrame>>>,
}

fn raw_nusb_worker<F: FnMut(&DeviceInfo) -> bool>(func: F, ctx: WireContext) -> Result<(), String> {
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

    let WireContext {
        outgoing,
        incoming,
        new_subs,
    } = ctx;

    let nctxt = Arc::new(NusbCtx {
        sub_map: Mutex::new(HashMap::new()),
    });

    tokio::task::spawn(out_worker(boq, outgoing));
    tokio::task::spawn(in_worker(biq, incoming, nctxt.clone()));
    tokio::task::spawn(sub_worker(new_subs, nctxt));

    Ok(())
}

/// Output worker, feeding frames to nusb.
///
/// TODO: We could maybe do something clever and have multiple "in flight" requests
/// with nusb, instead of doing it ~serially. If you are noticing degraded OUT endpoint
/// bandwidth (e.g. PC to USB device), lemme know and we can look at this.
async fn out_worker(mut boq: Queue<Vec<u8>>, mut rec: Receiver<RpcFrame>) {
    loop {
        let Some(msg) = rec.recv().await else {
            tracing::info!("Receiver Closed, existing out_worker");
            return;
        };

        boq.submit(msg.to_bytes());

        let send_res = boq.next_complete().await;
        if let Err(e) = send_res.status {
            tracing::error!("Output Queue Error: {e:?}, exiting");
            rec.close();
            return;
        }
    }
}

/// Input worker, getting frames from nusb
async fn in_worker(mut biq: Queue<RequestBuffer>, ctxt: Arc<HostContext>, nctxt: Arc<NusbCtx>) {
    let mut consecutive_errs = 0;

    for _ in 0..IN_FLIGHT_REQS {
        biq.submit(RequestBuffer::new(MAX_TRANSFER_SIZE));
    }

    loop {
        let res = biq.next_complete().await;

        if let Err(e) = res.status {
            consecutive_errs += 1;

            tracing::error!("In Worker error: {e:?}, consecutive: {consecutive_errs:?}");

            // Docs only recommend this for Stall, but it seems to work with
            // UNKNOWN on MacOS as well, todo: look into why!
            let fatal = if consecutive_errs <= MAX_STALL_RETRIES {
                tracing::warn!("Attempting stall recovery!");

                // Stall recovery shouldn't be used with in-flight requests, so
                // cancel them all. They'll still pop out of next_complete.
                biq.cancel_all();
                tracing::info!("Cancelled all in-flight requests");

                // Now we need to join all in flight requests
                for _ in 0..(IN_FLIGHT_REQS - 1) {
                    let res = biq.next_complete().await;
                    tracing::info!("Drain state: {:?}", res.status);
                }

                // Now we can mark the stall as clear
                match biq.clear_halt() {
                    Ok(()) => {
                        tracing::info!("Stall cleared! Rehydrating queue...");
                        for _ in 0..IN_FLIGHT_REQS {
                            biq.submit(RequestBuffer::new(MAX_TRANSFER_SIZE));
                        }
                        false
                    }
                    Err(e) => {
                        tracing::error!("Failed to clear stall: {e:?}, Fatal.");
                        true
                    }
                }
            } else {
                tracing::error!("Giving up after {consecutive_errs} errors in a row");
                true
            };

            if fatal {
                tracing::error!("Fatal Error, exiting");
                // TODO we should notify sub worker!
                ctxt.map.close();
                return;
            } else {
                tracing::info!("Potential recovery, resuming in_worker");
                continue;
            }
        }

        // replace the submission
        biq.submit(RequestBuffer::new(MAX_TRANSFER_SIZE));

        let Ok((hdr, body)) = extract_header_from_bytes(&res.data) else {
            tracing::warn!("Header decode error!");
            continue;
        };

        // If we get a good decode, clear the error flag
        if consecutive_errs != 0 {
            tracing::info!("Clearing consecutive error counter after good header decode");
            consecutive_errs = 0;
        }

        let mut handled = false;

        {
            let mut sg = nctxt.sub_map.lock().await;
            let key = hdr.key;

            // Remove if sending fails
            let rem = if let Some(m) = sg.get(&key) {
                handled = true;
                let frame = RpcFrame {
                    header: hdr.clone(),
                    body: body.to_vec(),
                };
                let res = m.try_send(frame);

                match res {
                    Ok(()) => {
                        tracing::debug!("Handled message via subscription");
                        false
                    }
                    Err(TrySendError::Full(_)) => {
                        tracing::error!("Subscription channel full! Message dropped.");
                        false
                    }
                    Err(TrySendError::Closed(_)) => true,
                }
            } else {
                false
            };

            if rem {
                tracing::debug!("Dropping subscription");
                sg.remove(&key);
            }
        }

        if handled {
            continue;
        }

        let frame = RpcFrame {
            header: hdr,
            body: body.to_vec(),
        };
        match ctxt.process_did_wake(frame) {
            Ok(true) => tracing::debug!("Handled message via map"),
            Ok(false) => tracing::debug!("Message not handled"),
            Err(ProcessError::Closed) => {
                tracing::warn!("Got process error, quitting");
                return;
            }
        }
    }
}

async fn sub_worker(mut new_subs: Receiver<SubInfo>, nctxt: Arc<NusbCtx>) {
    while let Some(sub) = new_subs.recv().await {
        let mut sg = nctxt.sub_map.lock().await;
        if let Some(_old) = sg.insert(sub.key, sub.tx) {
            tracing::warn!("Replacing old subscription for {:?}", sub.key);
        }
    }
}

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
    /// use serde::{Serialize, Deserialize};
    /// use postcard::experimental::schema::Schema;
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
    /// ).unwrap();
    /// ```
    pub fn try_new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Result<Self, String> {
        let (me, wire) = Self::new_manual(err_uri_path, outgoing_depth);
        raw_nusb_worker(func, wire)?;
        Ok(me)
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
    /// use serde::{Serialize, Deserialize};
    /// use postcard::experimental::schema::Schema;
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
    /// );
    /// ```
    pub fn new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Self {
        Self::try_new_raw_nusb(func, err_uri_path, outgoing_depth)
            .expect("should have found nusb device")
    }
}
