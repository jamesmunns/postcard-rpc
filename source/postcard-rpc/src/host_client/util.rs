// the contents of this file can probably be moved up to `mod.rs`
use std::{collections::HashMap, fmt::Debug, sync::Arc};

use maitake_sync::WaitQueue;
use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio::{
    select,
    sync::{
        mpsc::{error::TrySendError, Receiver, Sender},
        Mutex,
    },
};
use tracing::{debug, trace, warn};

use crate::{
    headered::extract_header_from_bytes,
    host_client::{
        HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext, WireRx, WireSpawn,
        WireTx,
    },
    Key,
};

pub(crate) type Subscriptions = HashMap<Key, Sender<RpcFrame>>;

/// A basic cancellation-token
///
/// Used to terminate (and signal termination of) worker tasks
#[derive(Clone)]
pub struct Stopper {
    inner: Arc<WaitQueue>,
}

impl Stopper {
    /// Create a new Stopper
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WaitQueue::new()),
        }
    }

    /// Wait until the stopper has been stopped.
    ///
    /// Once this completes, the stopper has been permanently stopped
    pub async fn wait_stopped(&self) {
        // This completes if we are awoken OR if the queue is closed: either
        // means we're cancelled
        let _ = self.inner.wait().await;
    }

    /// Have we been stopped?
    pub fn is_stopped(&self) -> bool {
        self.inner.is_closed()
    }

    /// Stop the stopper
    ///
    /// All current and future calls to [Self::wait_stopped] will complete immediately
    pub fn stop(&self) {
        self.inner.close();
    }
}

impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Generic HostClient logic, using the various Wire traits
    ///
    /// Typically used internally, but may also be used to implement HostClient
    /// over arbitrary transports.
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
        let (me, wire_ctx) = Self::new_manual_priv(err_uri_path, outgoing_depth);

        let WireContext {
            outgoing,
            incoming,
            new_subs,
        } = wire_ctx;

        let subscriptions: Arc<Mutex<Subscriptions>> = Arc::new(Mutex::new(Subscriptions::new()));

        sp.spawn(out_worker(tx, outgoing, me.stopper.clone()));
        sp.spawn(in_worker(
            rx,
            incoming,
            subscriptions.clone(),
            me.stopper.clone(),
        ));
        sp.spawn(sub_worker(new_subs, subscriptions, me.stopper.clone()));

        me
    }
}

/// Output worker, feeding frames to the `Client`.
async fn out_worker<W>(wire: W, rec: Receiver<RpcFrame>, stop: Stopper)
where
    W: WireTx,
    W::Error: Debug,
{
    let cancel_fut = stop.wait_stopped();
    let operate_fut = out_worker_inner(wire, rec);
    select! {
        _ = cancel_fut => {},
        _ = operate_fut => {
            // if WE exited, notify everyone else it's stoppin time
            stop.stop();
        },
    }
}

async fn out_worker_inner<W>(mut wire: W, mut rec: Receiver<RpcFrame>)
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
    wire: W,
    host_ctx: Arc<HostContext>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    stop: Stopper,
) where
    W: WireRx,
    W::Error: Debug,
{
    let cancel_fut = stop.wait_stopped();
    let operate_fut = in_worker_inner(wire, host_ctx, subscriptions);
    select! {
        _ = cancel_fut => {},
        _ = operate_fut => {
            // if WE exited, notify everyone else it's stoppin time
            stop.stop();
        },
    }
}

async fn in_worker_inner<W>(
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

async fn sub_worker(
    new_subs: Receiver<SubInfo>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    stop: Stopper,
) {
    let cancel_fut = stop.wait_stopped();
    let operate_fut = sub_worker_inner(new_subs, subscriptions);
    select! {
        _ = cancel_fut => {},
        _ = operate_fut => {
            // if WE exited, notify everyone else it's stoppin time
            stop.stop();
        },
    }
}

async fn sub_worker_inner(
    mut new_subs: Receiver<SubInfo>,
    subscriptions: Arc<Mutex<Subscriptions>>,
) {
    while let Some(sub) = new_subs.recv().await {
        let mut sub_guard = subscriptions.lock().await;
        if let Some(_old) = sub_guard.insert(sub.key, sub.tx) {
            warn!("Replacing old subscription for {:?}", sub.key);
        }
    }
}
