use std::collections::HashMap;

use nusb::{
    transfer::{Queue, RequestBuffer},
    DeviceInfo,
};
use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::{error::TrySendError, Receiver, Sender};
use tokio::sync::Mutex;

use crate::{headered::extract_header_from_bytes, Key};

use super::{HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext};
use std::sync::Arc;

pub(crate) const BULK_OUT_EP: u8 = 0x01;
pub(crate) const BULK_IN_EP: u8 = 0x81;

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

async fn in_worker(mut biq: Queue<RequestBuffer>, ctxt: Arc<HostContext>, nctxt: Arc<NusbCtx>) {
    let mut consecutive_errs = 0;
    const IN_FLIGHT_REQS: usize = 4;

    for _ in 0..IN_FLIGHT_REQS {
        biq.submit(RequestBuffer::new(1024));
    }

    loop {
        let res = biq.next_complete().await;

        if let Err(e) = res.status {
            consecutive_errs += 1;

            tracing::error!("In Worker error: {e:?}, consecutive: {consecutive_errs:?}");

            // Docs only recommend this for Stall, but it seems to work with
            // UNKNOWN on MacOS as well, todo: look into why!
            let fatal = if consecutive_errs <= 10 {
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
                            biq.submit(RequestBuffer::new(1024));
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

        consecutive_errs = 0;

        // replace the submission
        biq.submit(RequestBuffer::new(1024));

        let Ok((hdr, body)) = extract_header_from_bytes(&res.data) else {
            tracing::warn!("Header decode error!");
            continue;
        };

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
        if let Err(ProcessError::Closed) = ctxt.process(frame) {
            tracing::info!("Got process error, quitting");
            return;
        } else {
            tracing::debug!("Handled message via map");
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

/// # Constructor Methods
///
/// These methods are used to create a new [HostClient] instance for use with tokio serial and cobs encoding.
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    pub fn new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Self {
        let (me, wire) = Self::new_manual(err_uri_path, outgoing_depth);

        raw_nusb_worker(func, wire).unwrap();

        me
    }
}
