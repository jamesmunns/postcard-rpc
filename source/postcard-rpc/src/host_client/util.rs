// the contents of this file can probably be moved up to `mod.rs`
use core::fmt::Debug;
use std::{collections::HashMap, sync::Arc};

use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio::sync::{
    mpsc::{error::TrySendError, Receiver, Sender},
    Mutex,
};
use tracing::{debug, trace, warn};

use super::{
    HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext, WireRx, WireSpawn,
    WireTx,
};
use crate::{headered::extract_header_from_bytes, Key};

pub(crate) type Subscriptions = HashMap<Key, Sender<RpcFrame>>;

impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    pub fn new_with_wire<WTX, WRX, WSP>(
        tx: WTX,
        rx: WRX,
        mut sp: WSP,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Self
    where
        WTX: WireTx,
        WRX: WireRx,
        WSP: WireSpawn,
    {
        let (me, wire_ctx) = Self::new_manual(err_uri_path, outgoing_depth);

        let WireContext {
            outgoing,
            incoming,
            new_subs,
        } = wire_ctx;

        let subscriptions: Arc<Mutex<Subscriptions>> = Arc::new(Mutex::new(Subscriptions::new()));

        sp.spawn(out_worker(tx, outgoing));
        sp.spawn(in_worker(rx, incoming, subscriptions.clone()));
        sp.spawn(sub_worker(new_subs, subscriptions));

        me
    }
}

/// Output worker, feeding frames to the `Client`.
async fn out_worker<W>(mut wire: W, mut rec: Receiver<RpcFrame>)
where
    W: WireTx,
    W::Error: Debug,
{
    loop {
        let Some(msg) = rec.recv().await else {
            tracing::warn!("Receiver Closed, this could be bad");
            return;
        };
        if let Err(e) = wire.send(msg.to_bytes()).await {
            tracing::error!("Output Queue Error: {e:?}, exiting");
            return;
        }
    }
}

/// Input worker, getting frames from the `Client`
async fn in_worker<W>(
    mut wire: W,
    host_ctx: Arc<HostContext>,
    subscriptions: Arc<Mutex<Subscriptions>>,
) where
    W: WireRx,
    W::Error: Debug,
{
    loop {
        let Ok(res) = wire.receive().await else {
            warn!("in_worker: wire receive error, exiting");
            return;
        };

        let Ok((hdr, body)) = extract_header_from_bytes(&res) else {
            warn!("Header decode error!");
            continue;
        };

        trace!("in_worker received {hdr:?}");

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
                        trace!("Handled message via subscription");
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
