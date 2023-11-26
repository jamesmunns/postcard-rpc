// I'm just here so we can write integration tests

use std::collections::HashMap;

use postcard::experimental::schema::Schema;
use postcard_rpc::{WireHeader, host_client::{RpcFrame, HostClient, WireContext, ProcessError}, Endpoint, Key, Topic};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{sync::mpsc::{Receiver, Sender, channel}, select};

pub struct LocalServer {
    pub from_client: Receiver<RpcFrame>,
    pub to_client: Sender<RpcFrame>,
}

impl LocalServer {
    pub async fn reply<E: Endpoint>(&mut self, seq_no: u32, msg: &E::Response) -> Result<(), ()>
    where
        E::Response: Serialize,
    {
        self.to_client.send(RpcFrame {
            header: WireHeader {
                key: E::RESP_KEY,
                seq_no,
            },
            body: postcard::to_stdvec(msg)
            .unwrap(),
        })
        .await
        .map_err(drop)
    }

    pub async fn publish<T: Topic>(&mut self, seq_no: u32, msg: &T::Message) -> Result<(), ()>
    where
        T::Message: Serialize,
    {
        self.to_client.send(RpcFrame {
            header: WireHeader {
                key: T::TOPIC_KEY,
                seq_no,
            },
            body: postcard::to_stdvec(msg)
            .unwrap(),
        })
        .await
        .map_err(drop)
    }
}

pub struct LocalClient {
    pub to_server: Sender<RpcFrame>,
    pub from_server: Receiver<RpcFrame>,
}

pub fn local_setup<E>(bound: usize, err_uri_path: &str) -> (LocalServer, HostClient<E>)
where
    E: Schema + DeserializeOwned,
{
    let (srv_tx, srv_rx) = channel(bound);
    let (cli_tx, cli_rx) = channel(bound);
    let srv = LocalServer {
        from_client: cli_rx,
        to_client: srv_tx,
    };
    let cli = LocalClient {
        to_server: cli_tx,
        from_server: srv_rx,
    };
    let cli = make_client::<E>(cli, bound, err_uri_path);
    (srv, cli)
}

pub fn make_client<E>(
    cli: LocalClient,
    depth: usize,
    err_uri_path: &str,
) -> HostClient<E>
where
    E: Schema + DeserializeOwned,
{
    let (hcli, hcli_ctx) = HostClient::<E>::new_manual(err_uri_path, depth);
    tokio::task::spawn(wire_worker(cli, hcli_ctx));
    hcli
}

async fn wire_worker(mut cli: LocalClient, mut ctx: WireContext) {
    let mut subs: HashMap<Key, Sender<RpcFrame>> = HashMap::new();
    loop {
        // Wait for EITHER a serialized request, OR some data from the embedded device
        select! {
            sub = ctx.new_subs.recv() => {
                let Some(si) = sub else {
                    return;
                };

                subs.insert(si.key, si.tx);
            }
            out = ctx.outgoing.recv() => {
                let Some(msg) = out else {
                    return;
                };
                if cli.to_server.send(msg).await.is_err() {
                    return;
                }
            }
            inc = cli.from_server.recv() => {
                let Some(msg) = inc else {
                    return;
                };
                // Give priority to subscriptions. TBH I only do this because I know a hashmap
                // lookup is cheaper than a waitmap search.
                let key = msg.header.key;
                if let Some(tx) = subs.get_mut(&key) {
                    // Yup, we have a subscription
                    if tx.send(msg).await.is_err() {
                        // But if sending failed, the listener is gone, so drop it
                        subs.remove(&key);
                    }
                } else {
                    // Wake the given sequence number. If the WaitMap is closed, we're done here
                    if let Err(ProcessError::Closed) = ctx.incoming.process(msg) {
                        return;
                    }
                }
            }
        }
    }
}
