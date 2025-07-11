use core::time::Duration;
// the contents of this file can probably be moved up to `mod.rs`
use std::{fmt::Debug, sync::Arc};

use maitake_sync::WaitQueue;
use postcard_schema::Schema;
use serde::de::DeserializeOwned;
use tokio::{
    select,
    sync::{broadcast, mpsc, Mutex},
};
use tracing::{debug, trace, warn};

use crate::{
    header::{VarHeader, VarKey, VarSeqKind},
    host_client::{
        HostClient, HostContext, ProcessError, RpcFrame, WireContext, WireRx, WireSpawn, WireTx,
    },
    Key,
};

#[derive(Default, Debug)]
pub(crate) struct Subscriptions {
    pub(crate) exclusive_list: Vec<(Key, mpsc::Sender<RpcFrame>)>,
    pub(crate) broadcast_list: Vec<(Key, broadcast::Sender<RpcFrame>)>,
    pub(crate) stopped: bool,
}

/// A basic cancellation-token
///
/// Used to terminate (and signal termination of) worker tasks
#[derive(Clone)]
pub struct Stopper {
    inner: Arc<WaitQueue>,
}

impl core::fmt::Debug for Stopper {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Stopper")
            .field("is_stopped", &self.is_stopped())
            .finish_non_exhaustive()
    }
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

impl Default for Stopper {
    fn default() -> Self {
        Self::new()
    }
}

/// HostClient configuration
pub struct HostClientConfig<'c> {
    /// The sequence kind to use
    pub seq_kind: VarSeqKind,

    /// The URI path to use for error messages
    pub err_uri_path: &'c str,

    /// The depth of the outgoing queue
    pub outgoing_depth: usize,

    /// Timeout to use before dropping a message if a subscribe channel is full.
    ///
    /// Does not apply to subscribe_multi channels.
    pub subscriber_timeout_if_full: Duration,
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
        sp: WSP,
        seq_kind: VarSeqKind,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Self
    where
        WTX: WireTx,
        WRX: WireRx,
        WSP: WireSpawn,
    {
        let config = HostClientConfig {
            seq_kind,
            err_uri_path,
            outgoing_depth,
            subscriber_timeout_if_full: Duration::ZERO,
        };

        Self::new_with_wire_and_config(tx, rx, sp, &config)
    }

    /// Generic HostClient logic, using the various Wire traits
    ///
    /// Typically used internally, but may also be used to implement HostClient
    /// over arbitrary transports.
    pub fn new_with_wire_and_config<WTX, WRX, WSP>(
        tx: WTX,
        rx: WRX,
        mut sp: WSP,
        config: &HostClientConfig<'_>,
    ) -> Self
    where
        WTX: WireTx,
        WRX: WireRx,
        WSP: WireSpawn,
    {
        let (me, wire_ctx) = Self::new_manual_priv(config);

        let WireContext { outgoing, incoming } = wire_ctx;

        sp.spawn(out_worker(tx, outgoing, me.stopper.clone()));
        sp.spawn(in_worker(
            rx,
            incoming,
            me.subscriptions.clone(),
            me.stopper.clone(),
        ));

        me
    }
}

/// Output worker, feeding frames to the `Client`.
async fn out_worker<W>(wire: W, rec: mpsc::Receiver<RpcFrame>, stop: Stopper)
where
    W: WireTx,
    W::Error: Debug,
{
    let cancel_fut = stop.wait_stopped();
    let operate_fut = out_worker_inner(wire, rec);
    select! {
        biased;
        _ = cancel_fut => {},
        _ = operate_fut => {
            // if WE exited, notify everyone else it's stoppin time
            stop.stop();
        },
    }
}

async fn out_worker_inner<W>(mut wire: W, mut rec: mpsc::Receiver<RpcFrame>)
where
    W: WireTx,
    W::Error: Debug,
{
    loop {
        let Some(msg) = rec.recv().await else {
            tracing::info!("Receiver Closed");
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
    let operate_fut = in_worker_inner(wire, host_ctx, subscriptions.clone());
    select! {
        biased;
        _ = cancel_fut => {},
        _ = operate_fut => {
            // if WE exited, notify everyone else it's stoppin time
            stop.stop();
        },
    }
    // If we stop, purge the subscription list so that it is clear that no more messages are coming
    // TODO: Have a "stopped" flag to prevent later additions (e.g. sub after store?)
    let mut guard = subscriptions.lock().await;
    guard.stopped = true;
    guard.exclusive_list.clear();
    guard.broadcast_list.clear();
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

        let Some((hdr, body)) = VarHeader::take_from_slice(&res) else {
            warn!("Header decode error!");
            continue;
        };

        trace!("in_worker received {hdr:?}");

        let mut handled = false;

        {
            let mut subs_guard = subscriptions.lock().await;
            let key = hdr.key;

            // Remove if sending fails
            //
            // First, check the broadcast channels
            let remove_mul_sub = if let Some((_h, m)) = subs_guard
                .broadcast_list
                .iter()
                .find(|(k, _)| VarKey::Key8(*k) == key)
            {
                handled = true;
                let frame = RpcFrame {
                    header: hdr,
                    body: body.to_vec(),
                };
                let res = m.send(frame);

                match res {
                    Ok(_) => {
                        trace!("Handled message via subscription");
                        false
                    }
                    // A SendError means that there are no more receivers
                    Err(broadcast::error::SendError(_)) => true,
                }
            } else {
                false
            };

            let remove_exl_sub = if let Some((_h, m)) = subs_guard
                .exclusive_list
                .iter()
                .find(|(k, _)| VarKey::Key8(*k) == key)
            {
                handled = true;
                let frame = RpcFrame {
                    header: hdr,
                    body: body.to_vec(),
                };

                let res = m.try_send(frame);

                match res {
                    Ok(()) => {
                        trace!("Handled message via subscription");
                        false
                    }
                    Err(mpsc::error::TrySendError::Full(_))
                        if host_ctx.subscription_timeout.is_zero() =>
                    {
                        tracing::error!("Subscription channel full! Message dropped.");
                        false
                    }
                    Err(mpsc::error::TrySendError::Full(frame)) => {
                        tokio::select! {
                            // send returns an error if the channel is closed
                            r = m.send(frame) => r.is_err(),
                            _ = tokio::time::sleep(host_ctx.subscription_timeout) => {
                                tracing::error!("Subscription channel full! Message dropped.");
                                false
                            }
                        }
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => true,
                }
            } else {
                false
            };

            if remove_exl_sub {
                debug!("Dropping exclusive subscription");
                subs_guard
                    .exclusive_list
                    .retain(|(k, _)| VarKey::Key8(*k) != key);
            }
            if remove_mul_sub {
                debug!("Dropping multi subscription");
                subs_guard
                    .broadcast_list
                    .retain(|(k, _)| VarKey::Key8(*k) != key);
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
