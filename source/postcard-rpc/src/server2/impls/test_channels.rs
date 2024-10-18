//! Implementation that uses channels for local testing

use core::{convert::Infallible, future::Future};

use crate::server2::{AsWireRxErrorKind, AsWireTxErrorKind, Server, WireRx, WireRxErrorKind, WireSpawn, WireTx, WireTxErrorKind};
use tokio::sync::mpsc;

pub struct NewChannelServer {
    pub server: Server<ChannelWireTx, ChannelWireRx, Box<[u8]>>,
    pub client_tx: mpsc::Sender<Vec<u8>>,
    pub client_rx: mpsc::Receiver<Vec<u8>>,
}

pub fn new_channel_server(bound: usize, buf: usize) -> NewChannelServer {
    let (client_tx, server_rx) = mpsc::channel(bound);
    let (server_tx, client_rx) = mpsc::channel(bound);

    let cwrx = ChannelWireRx {
        rx: server_rx,
    };
    let cwtx = ChannelWireTx {
        tx: server_tx,
    };
    let buf = vec![0; buf];
    let server = Server::new(&cwtx, cwrx, buf.into_boxed_slice());

    NewChannelServer {
        server,
        client_tx,
        client_rx,
    }
}

#[derive(Clone)]
pub struct ChannelWireTx {
    tx: mpsc::Sender<Vec<u8>>,
}

impl WireTx for ChannelWireTx {
    type Error = ChannelWireTxError;

    async fn send<T: serde::Serialize + ?Sized>(
        &self,
        hdr: crate::WireHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut hdr_ser = postcard::to_stdvec(&hdr).unwrap();
        let bdy_ser = postcard::to_stdvec(msg).unwrap();
        hdr_ser.extend_from_slice(&bdy_ser);
        self.tx.send(hdr_ser).await.map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let buf = buf.to_vec();
        self.tx.send(buf).await.map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ChannelWireRxError {
    ChannelClosed,
    MessageTooLarge,
}

impl AsWireRxErrorKind for ChannelWireRxError {
    fn as_kind(&self) -> WireRxErrorKind {
        match self {
            ChannelWireRxError::ChannelClosed => WireRxErrorKind::ConnectionClosed,
            ChannelWireRxError::MessageTooLarge => WireRxErrorKind::ReceivedMessageTooLarge,
        }
    }
}

#[derive(Debug)]
pub enum ChannelWireTxError {
    ChannelClosed,
}

impl AsWireTxErrorKind for ChannelWireTxError {
    fn as_kind(&self) -> WireTxErrorKind {
        match self {
            ChannelWireTxError::ChannelClosed => WireTxErrorKind::ConnectionClosed,
        }
    }
}

pub struct ChannelWireRx {
    rx: mpsc::Receiver<Vec<u8>>,
}

impl WireRx for ChannelWireRx {
    type Error = ChannelWireRxError;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        // todo: some kind of receive_owned?
        let msg = self.rx.recv().await;
        let msg = msg.ok_or(ChannelWireRxError::ChannelClosed)?;
        let out = buf.get_mut(..msg.len()).ok_or(ChannelWireRxError::MessageTooLarge)?;
        out.copy_from_slice(&msg);
        Ok(out)
    }
}

// todo: just use a standard tokio impl?
#[derive(Clone)]
pub struct ChannelWireSpawn {}

impl WireSpawn for ChannelWireSpawn {
    type Error = Infallible;

    type Info = ();

    fn info(&self) -> &Self::Info {
        &()
    }
}

pub fn tokio_spawn<Sp, F>(_sp: &Sp, fut: F)
where
    Sp: WireSpawn<Error = Infallible, Info = ()>,
    F: Future<Output = ()> + 'static + Send,
{
    tokio::task::spawn(fut);
}

#[cfg(test)]
mod test {
    #![allow(dead_code)]

    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use postcard_schema::Schema;
    use serde::{Deserialize, Serialize};

    use crate::{define_dispatch2, endpoint, headered::extract_header_from_bytes, target_server::SpawnContext, Endpoint, WireHeader};

    use super::*;

    #[derive(Serialize, Deserialize, Schema)]
    pub struct AReq(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct AResp(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct GReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct GResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct DReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct DResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct EReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct EResp;

    endpoint!(AlphaEndpoint, AReq, AResp, "alpha");
    endpoint!(BetaEndpoint, BReq, BResp, "beta");
    endpoint!(GammaEndpoint, GReq, GResp, "gamma");
    endpoint!(DeltaEndpoint, DReq, DResp, "delta");
    endpoint!(EpsilonEndpoint, EReq, EResp, "epsilon");

    pub struct TestContext {
        pub ctr: Arc<AtomicUsize>,
    }

    impl SpawnContext for TestContext {
        type SpawnCtxt = TestSpawnContext;

        fn spawn_ctxt(&mut self) -> Self::SpawnCtxt {
            TestSpawnContext { ctr: self.ctr.clone() }
        }
    }

    pub struct TestSpawnContext {
        pub ctr: Arc<AtomicUsize>,
    }

    define_dispatch2! {
        dispatcher: SingleDispatcher<WireTx = ChannelWireTx, WireSpawn = ChannelWireSpawn, Context = TestContext>;
        spawn_fn: embassy_spawn;
        AlphaEndpoint => async test_alpha_handler,
    }

    async fn test_alpha_handler(
        context: &mut TestContext,
        _header: WireHeader,
        body: AReq,
    ) -> AResp {
        context.ctr.fetch_add(1, Ordering::Relaxed);
        AResp(body.0)
    }

    #[tokio::test]
    async fn smoke() {
        let NewChannelServer { mut server, client_tx, mut client_rx } = new_channel_server(16, 1024);
        let dispatcher = SingleDispatcher::new(TestContext { ctr: Arc::new(AtomicUsize::new(0)) }, ChannelWireSpawn {  });
        tokio::task::spawn(async move {
            server.run(dispatcher).await;
        });

        // manually build request
        let mut msg = postcard::to_stdvec(&WireHeader { key: AlphaEndpoint::REQ_KEY, seq_no: 123 }).unwrap();
        let body = postcard::to_stdvec(&AReq(42)).unwrap();
        msg.extend_from_slice(&body);
        client_tx.send(msg).await.unwrap();
        let resp = client_rx.recv().await.unwrap();

        // manually extract response
        let (hdr, body) = extract_header_from_bytes(&resp).unwrap();
        let resp = postcard::from_bytes::<<AlphaEndpoint as Endpoint>::Response>(body).unwrap();
        assert_eq!(resp.0, 42);
        assert_eq!(hdr.key, AlphaEndpoint::RESP_KEY);
        assert_eq!(hdr.seq_no, 123);
    }
}
