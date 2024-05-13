use std::{collections::HashMap, sync::Arc};

// TODO running inside dioxus requires using dioxus spawn, but
// this isn't strictly a *WebUSB* requirement.
// We could also be e.g. running inside a naked `trunk` app.
// Also this is the only part of this impl that isn't a generic vec pipe 🤔
use dioxus_core::prelude::*;
use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio::sync::{
    mpsc::{error::TrySendError, Receiver, Sender},
    Mutex,
};
use tracing::{debug, warn};

use super::{HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext};
use crate::{headered::extract_header_from_bytes, Key};

// TODO type shared with nusb transport
pub(crate) type Subscriptions = HashMap<Key, Sender<RpcFrame>>;

fn usb_worker(
    wire_in: Receiver<Vec<u8>>,
    wire_out: Sender<Vec<u8>>,
    ctx: WireContext,
) -> Result<(), String> {
    let WireContext {
        outgoing,
        incoming,
        new_subs,
    } = ctx;

    let subscriptions: Arc<Mutex<Subscriptions>> = Arc::new(Mutex::new(Subscriptions::new()));

    spawn(out_worker(wire_out, outgoing));
    spawn(in_worker(wire_in, incoming, subscriptions.clone()));
    spawn(sub_worker(new_subs, subscriptions));

    Ok(())
}

/// Output worker, feeding frames to webusb.
async fn out_worker(wire_out: Sender<Vec<u8>>, mut rec: Receiver<RpcFrame>) {
    loop {
        let Some(msg) = rec.recv().await else {
            tracing::warn!("Receiver Closed, this could be bad");
            return;
        };
        if let Err(e) = wire_out.send(msg.to_bytes()).await {
            tracing::error!("Output Queue Error: {e:?}, exiting");
            return;
        }
    }
}

/// Input worker, getting frames from webusb
async fn in_worker(
    mut wire_in: Receiver<Vec<u8>>,
    host_ctx: Arc<HostContext>,
    subscriptions: Arc<Mutex<Subscriptions>>,
) {
    loop {
        let Some(res) = wire_in.recv().await else {
            warn!("in_worker: receiver channel closed, exiting");
            return;
        };

        let Ok((hdr, body)) = extract_header_from_bytes(&res) else {
            warn!("Header decode error!");
            continue;
        };

        debug!("in_worker received {hdr:?}");

        let mut handled = false;

        {
            let mut subs_guard = subscriptions.lock().await;
            let key = hdr.key;

            // Remove if sending fails
            let remove_sub = if let Some(m) = subs_guard.get(&key) {
                handled = true;
                let frame = RpcFrame {
                    header: hdr.clone(),
                    body: body.to_vec(),
                };
                let res = m.try_send(frame);

                match res {
                    Ok(()) => {
                        debug!("Handled message via subscription");
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

            if remove_sub {
                debug!("Dropping subscription");
                subs_guard.remove(&key);
            }
        }

        if handled {
            continue;
        }

        let frame = RpcFrame {
            header: hdr,
            body: body.to_vec(),
        };

        match host_ctx.process_did_wake(frame) {
            Ok(true) => debug!("Handled message via map"),
            Ok(false) => debug!("Message not handled"),
            Err(ProcessError::Closed) => {
                warn!("Got process error, quitting");
                return;
            }
        }
    }
}

async fn sub_worker(mut new_subs: Receiver<SubInfo>, subscriptions: Arc<Mutex<Subscriptions>>) {
    while let Some(sub) = new_subs.recv().await {
        let mut sub_guard = subscriptions.lock().await;
        if let Some(_old) = sub_guard.insert(sub.key, sub.tx) {
            warn!("Replacing old subscription for {:?}", sub.key);
        }
    }
}

impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    pub fn new_webusb(
        wire_in: Receiver<Vec<u8>>,
        wire_out: Sender<Vec<u8>>,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Result<Self, String> {
        let (me, wire_ctx) = Self::new_manual(err_uri_path, outgoing_depth);
        usb_worker(wire_in, wire_out, wire_ctx)?;
        Ok(me)
    }
}
