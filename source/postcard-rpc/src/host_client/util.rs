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

use super::{Client, HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext};
use crate::{headered::extract_header_from_bytes, Key};

pub(crate) type Subscriptions = HashMap<Key, Sender<RpcFrame>>;

impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    pub fn new_with_client<C>(client: C, err_uri_path: &str, outgoing_depth: usize) -> Self
    where
        C: Client,
        C::Error: Debug,
    {
        let (me, wire_ctx) = Self::new_manual(err_uri_path, outgoing_depth);
        spawn_workers(client, wire_ctx);
        me
    }
}

fn spawn_workers<C>(client: C, ctx: WireContext)
where
    C: Client,
    C::Error: Debug,
{
    let WireContext {
        outgoing,
        incoming,
        new_subs,
    } = ctx;

    let subscriptions: Arc<Mutex<Subscriptions>> = Arc::new(Mutex::new(Subscriptions::new()));

    client.spawn(out_worker(client.clone(), outgoing));
    client.spawn(in_worker(client.clone(), incoming, subscriptions.clone()));
    client.spawn(sub_worker(new_subs, subscriptions));
}

/// Output worker, feeding frames to webusb.
async fn out_worker<C>(client: C, mut rec: Receiver<RpcFrame>)
where
    C: Client,
    C::Error: Debug,
{
    loop {
        let Some(msg) = rec.recv().await else {
            tracing::warn!("Receiver Closed, this could be bad");
            return;
        };
        if let Err(e) = client.send(msg.to_bytes()).await {
            tracing::error!("Output Queue Error: {e:?}, exiting");
            return;
        }
    }
}

/// Input worker, getting frames from webusb
async fn in_worker<C>(
    client: C,
    host_ctx: Arc<HostContext>,
    subscriptions: Arc<Mutex<Subscriptions>>,
) where
    C: Client,
    C::Error: Debug,
{
    loop {
        let Ok(res) = client.receive().await else {
            warn!("in_worker: client receive error, exiting");
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
