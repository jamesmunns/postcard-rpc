//! Test utilities for doctests and integration tests

use core::{fmt::Display, future::Future};

use crate::host_client::util::Stopper;
use crate::{
    headered::extract_header_from_bytes,
    host_client::{HostClient, RpcFrame, WireRx, WireSpawn, WireTx},
    Endpoint, Topic, WireHeader,
};
use postcard::experimental::schema::Schema;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    select,
    sync::mpsc::{channel, Receiver, Sender},
};

pub struct LocalRx {
    fake_error: Stopper,
    from_server: Receiver<Vec<u8>>,
}
pub struct LocalTx {
    fake_error: Stopper,
    to_server: Sender<Vec<u8>>,
}
pub struct LocalSpawn;
pub struct LocalFakeServer {
    fake_error: Stopper,
    pub from_client: Receiver<Vec<u8>>,
    pub to_client: Sender<Vec<u8>>,
}

impl LocalFakeServer {
    pub async fn recv_from_client(&mut self) -> Result<RpcFrame, LocalError> {
        let msg = self.from_client.recv().await.ok_or(LocalError::TxClosed)?;
        let Ok((hdr, body)) = extract_header_from_bytes(&msg) else {
            return Err(LocalError::BadFrame);
        };
        Ok(RpcFrame {
            header: hdr,
            body: body.to_vec(),
        })
    }

    pub async fn reply<E: Endpoint>(
        &mut self,
        seq_no: u32,
        data: &E::Response,
    ) -> Result<(), LocalError>
    where
        E::Response: Serialize,
    {
        let frame = RpcFrame {
            header: WireHeader {
                key: E::RESP_KEY,
                seq_no,
            },
            body: postcard::to_stdvec(data).unwrap(),
        };
        self.to_client
            .send(frame.to_bytes())
            .await
            .map_err(|_| LocalError::RxClosed)
    }

    pub async fn publish<T: Topic>(
        &mut self,
        seq_no: u32,
        data: &T::Message,
    ) -> Result<(), LocalError>
    where
        T::Message: Serialize,
    {
        let frame = RpcFrame {
            header: WireHeader {
                key: T::TOPIC_KEY,
                seq_no,
            },
            body: postcard::to_stdvec(data).unwrap(),
        };
        self.to_client
            .send(frame.to_bytes())
            .await
            .map_err(|_| LocalError::RxClosed)
    }

    pub fn cause_fatal_error(&self) {
        self.fake_error.stop();
    }
}

#[derive(Debug, PartialEq)]
pub enum LocalError {
    RxClosed,
    TxClosed,
    BadFrame,
    FatalError,
}

impl Display for LocalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        <Self as core::fmt::Debug>::fmt(self, f)
    }
}

impl std::error::Error for LocalError {}

impl WireRx for LocalRx {
    type Error = LocalError;

    #[allow(clippy::manual_async_fn)]
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        async {
            // This is not usually necessary - HostClient machinery takes care of listening
            // to the stopper, but we have an EXTRA one to simulate I/O failure
            let recv_fut = self.from_server.recv();
            let error_fut = self.fake_error.wait_stopped();

            // Before we await, do a quick check to see if an error occured, this way
            // recv can't accidentally win the select
            if self.fake_error.is_stopped() {
                return Err(LocalError::FatalError);
            }

            select! {
                recv = recv_fut => recv.ok_or(LocalError::RxClosed),
                _err = error_fut => Err(LocalError::FatalError),
            }
        }
    }
}

impl WireTx for LocalTx {
    type Error = LocalError;

    #[allow(clippy::manual_async_fn)]
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send {
        async {
            // This is not usually necessary - HostClient machinery takes care of listening
            // to the stopper, but we have an EXTRA one to simulate I/O failure
            let send_fut = self.to_server.send(data);
            let error_fut = self.fake_error.wait_stopped();

            // Before we await, do a quick check to see if an error occured, this way
            // send can't accidentally win the select
            if self.fake_error.is_stopped() {
                return Err(LocalError::FatalError);
            }

            select! {
                send = send_fut => send.map_err(|_| LocalError::TxClosed),
                _err = error_fut => Err(LocalError::FatalError),
            }
        }
    }
}

impl WireSpawn for LocalSpawn {
    fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) {
        tokio::task::spawn(fut);
    }
}

/// This function creates a directly-linked Server and Client.
///
/// This is useful for testing and demonstrating server/client behavior,
/// without actually requiring an external device.
pub fn local_setup<E>(bound: usize, err_uri_path: &str) -> (LocalFakeServer, HostClient<E>)
where
    E: Schema + DeserializeOwned,
{
    let (c2s_tx, c2s_rx) = channel(bound);
    let (s2c_tx, s2c_rx) = channel(bound);

    // NOTE: the normal HostClient machinery has it's own Stopper used for signalling
    // errors, this is an EXTRA stopper we use to simulate the error occurring, like
    // if our USB device disconnected or the serial port was closed
    let fake_error = Stopper::new();

    let client = HostClient::<E>::new_with_wire(
        LocalTx {
            to_server: c2s_tx,
            fake_error: fake_error.clone(),
        },
        LocalRx {
            from_server: s2c_rx,
            fake_error: fake_error.clone(),
        },
        LocalSpawn,
        err_uri_path,
        bound,
    );

    let lfs = LocalFakeServer {
        from_client: c2s_rx,
        to_client: s2c_tx,
        fake_error: fake_error.clone(),
    };

    (lfs, client)
}
