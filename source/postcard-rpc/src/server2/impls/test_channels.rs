//! Implementation that uses channels for local testing

use core::{convert::Infallible, future::Future};

use crate::server2::{
    AsWireRxErrorKind, AsWireTxErrorKind, WireRx, WireRxErrorKind, WireSpawn, WireTx,
    WireTxErrorKind,
};
use tokio::sync::mpsc;

//////////////////////////////////////////////////////////////////////////////
// DISPATCH IMPL
//////////////////////////////////////////////////////////////////////////////

pub mod dispatch_impl {
    pub struct Settings {
        pub tx: WireTxImpl,
        pub rx: WireRxImpl,
        pub buf: usize,
    }

    pub type WireTxImpl = super::ChannelWireTx;
    pub type WireRxImpl = super::ChannelWireRx;
    pub type WireSpawnImpl = super::ChannelWireSpawn;
    pub type WireRxBuf = Box<[u8]>;

    use crate::server2::{Dispatch2, Server};

    pub use super::tokio_spawn as spawn_fn;

    pub fn new_server<D>(
        dispatch: D,
        settings: Settings,
    ) -> crate::server2::Server<WireTxImpl, WireRxImpl, WireRxBuf, D>
    where
        D: Dispatch2<Tx = WireTxImpl>,
    {
        let buf = vec![0; settings.buf];
        Server::new(&settings.tx, settings.rx, buf.into_boxed_slice(), dispatch)
    }
}

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

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
        self.tx
            .send(hdr_ser)
            .await
            .map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let buf = buf.to_vec();
        self.tx
            .send(buf)
            .await
            .map_err(|_| ChannelWireTxError::ChannelClosed)?;
        Ok(())
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

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

pub struct ChannelWireRx {
    rx: mpsc::Receiver<Vec<u8>>,
}

impl WireRx for ChannelWireRx {
    type Error = ChannelWireRxError;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        // todo: some kind of receive_owned?
        let msg = self.rx.recv().await;
        let msg = msg.ok_or(ChannelWireRxError::ChannelClosed)?;
        let out = buf
            .get_mut(..msg.len())
            .ok_or(ChannelWireRxError::MessageTooLarge)?;
        out.copy_from_slice(&msg);
        Ok(out)
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

//////////////////////////////////////////////////////////////////////////////
// SPAWN
//////////////////////////////////////////////////////////////////////////////

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

pub fn tokio_spawn<Sp, F>(_sp: &Sp, fut: F) -> Result<(), Sp::Error>
where
    Sp: WireSpawn<Error = Infallible, Info = ()>,
    F: Future<Output = ()> + 'static + Send,
{
    tokio::task::spawn(fut);
    Ok(())
}

#[cfg(test)]
mod test {
    #![allow(dead_code)]

    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };
    use std::{sync::Arc, time::Instant};

    use postcard_schema::Schema;
    use serde::{Deserialize, Serialize};
    use tokio::task::yield_now;

    use crate::{
        define_dispatch2, endpoints,
        headered::extract_header_from_bytes,
        server2::{Sender, SpawnContext},
        topics, Endpoint, Topic, WireHeader,
    };

    use super::*;

    #[derive(Serialize, Deserialize, Schema)]
    pub struct AReq(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct AResp(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BReq(pub u16);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BResp(pub u32);
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
    #[derive(Serialize, Deserialize, Schema)]
    pub struct ZMsg(pub i16);

    endpoints! {
        list = ENDPOINT_LIST;
        | EndpointTy        | RequestTy     | ResponseTy    | Path              |
        | ----------        | ---------     | ----------    | ----              |
        | AlphaEndpoint     | AReq          | AResp         | "alpha"           |
        | BetaEndpoint      | BReq          | BResp         | "beta"            |
        | GammaEndpoint     | GReq          | GResp         | "gamma"           |
        | DeltaEndpoint     | DReq          | DResp         | "delta"           |
        | EpsilonEndpoint   | EReq          | EResp         | "epsilon"         |
    }

    topics! {
        list = TOPICS_IN_LIST;
        | TopicTy           | MessageTy     | Path              |
        | ----------        | ---------     | ----              |
        | ZetaTopic1        | ZMsg          | "zeta1"           |
        | ZetaTopic2        | ZMsg          | "zeta2"           |
        | ZetaTopic3        | ZMsg          | "zeta3"           |
    }

    pub struct TestContext {
        pub ctr: Arc<AtomicUsize>,
        pub topic_ctr: Arc<AtomicUsize>,
    }

    pub struct TestSpawnContext {
        pub ctr: Arc<AtomicUsize>,
        pub topic_ctr: Arc<AtomicUsize>,
    }

    impl SpawnContext for TestContext {
        type SpawnCtxt = TestSpawnContext;

        fn spawn_ctxt(&mut self) -> Self::SpawnCtxt {
            TestSpawnContext {
                ctr: self.ctr.clone(),
                topic_ctr: self.topic_ctr.clone(),
            }
        }
    }

    // TODO: How to do module path concat?
    use crate::server2::impls::test_channels::dispatch_impl::{
        new_server, spawn_fn, Settings, WireSpawnImpl, WireTxImpl,
    };

    define_dispatch2! {
        app: SingleDispatcher;
        spawn_fn: spawn_fn;
        tx_impl: WireTxImpl;
        spawn_impl: WireSpawnImpl;
        context: TestContext;

        endpoints: {
            list: ENDPOINT_LIST;

            | EndpointTy        | kind      | handler               |
            | ----------        | ----      | -------               |
            | AlphaEndpoint     | async     | test_alpha_handler    |
            | BetaEndpoint      | spawn     | test_beta_handler     |
        };
        topics_in: {
            list: TOPICS_IN_LIST;

            | TopicTy           | kind      | handler               |
            | ----------        | ----      | -------               |
            | ZetaTopic1        | blocking  | test_zeta_blocking    |
            | ZetaTopic2        | async     | test_zeta_async       |
            | ZetaTopic3        | spawn     | test_zeta_spawn       |
        };
    }

    fn test_zeta_blocking(
        context: &mut TestContext,
        _header: WireHeader,
        _body: ZMsg,
        _out: &Sender<ChannelWireTx>,
    ) {
        context.topic_ctr.fetch_add(1, Ordering::Relaxed);
    }

    async fn test_zeta_async(
        context: &mut TestContext,
        _header: WireHeader,
        _body: ZMsg,
        _out: &Sender<ChannelWireTx>,
    ) {
        context.topic_ctr.fetch_add(1, Ordering::Relaxed);
    }

    async fn test_zeta_spawn(
        context: TestSpawnContext,
        _header: WireHeader,
        _body: ZMsg,
        _out: Sender<ChannelWireTx>,
    ) {
        context.topic_ctr.fetch_add(1, Ordering::Relaxed);
    }

    async fn test_alpha_handler(
        context: &mut TestContext,
        _header: WireHeader,
        body: AReq,
    ) -> AResp {
        context.ctr.fetch_add(1, Ordering::Relaxed);
        AResp(body.0)
    }

    async fn test_beta_handler(
        context: TestSpawnContext,
        header: WireHeader,
        body: BReq,
        out: Sender<ChannelWireTx>,
    ) {
        context.ctr.fetch_add(1, Ordering::Relaxed);
        let _ = out
            .reply::<BetaEndpoint>(header.seq_no, &BResp(body.0.into()))
            .await;
    }

    #[tokio::test]
    async fn smoke() {
        let (client_tx, server_rx) = mpsc::channel(16);
        let (server_tx, mut client_rx) = mpsc::channel(16);

        let cwrx = ChannelWireRx { rx: server_rx };
        let cwtx = ChannelWireTx { tx: server_tx };

        let topic_ctr = Arc::new(AtomicUsize::new(0));

        let app = SingleDispatcher::new(
            TestContext {
                ctr: Arc::new(AtomicUsize::new(0)),
                topic_ctr: topic_ctr.clone(),
            },
            ChannelWireSpawn {},
        );
        let mut server = new_server(
            app,
            Settings {
                tx: cwtx,
                rx: cwrx,
                buf: 1024,
            },
        );
        tokio::task::spawn(async move {
            server.run().await;
        });

        // manually build request - Alpha
        let mut msg = postcard::to_stdvec(&WireHeader {
            key: AlphaEndpoint::REQ_KEY,
            seq_no: 123,
        })
        .unwrap();
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

        // manually build request - Beta
        let mut msg = postcard::to_stdvec(&WireHeader {
            key: BetaEndpoint::REQ_KEY,
            seq_no: 234,
        })
        .unwrap();
        let body = postcard::to_stdvec(&BReq(1000)).unwrap();
        msg.extend_from_slice(&body);
        client_tx.send(msg).await.unwrap();
        let resp = client_rx.recv().await.unwrap();

        // manually extract response
        let (hdr, body) = extract_header_from_bytes(&resp).unwrap();
        let resp = postcard::from_bytes::<<BetaEndpoint as Endpoint>::Response>(body).unwrap();
        assert_eq!(resp.0, 1000);
        assert_eq!(hdr.key, BetaEndpoint::RESP_KEY);
        assert_eq!(hdr.seq_no, 234);

        // blocking topic handler
        for i in 0..3 {
            let mut msg = postcard::to_stdvec(&WireHeader {
                key: ZetaTopic1::TOPIC_KEY,
                seq_no: i,
            })
            .unwrap();
            let body = postcard::to_stdvec(&ZMsg(456)).unwrap();
            msg.extend_from_slice(&body);
            client_tx.send(msg).await.unwrap();

            let start = Instant::now();
            let mut good = false;
            while start.elapsed() < Duration::from_millis(100) {
                let ct = topic_ctr.load(Ordering::Relaxed);
                if ct == (i + 1) as usize {
                    good = true;
                    break;
                } else {
                    yield_now().await
                }
            }
            assert!(good);
        }

        // async topic handler
        for i in 0..3 {
            let mut msg = postcard::to_stdvec(&WireHeader {
                key: ZetaTopic2::TOPIC_KEY,
                seq_no: i,
            })
            .unwrap();
            let body = postcard::to_stdvec(&ZMsg(456)).unwrap();
            msg.extend_from_slice(&body);
            client_tx.send(msg).await.unwrap();

            let start = Instant::now();
            let mut good = false;
            while start.elapsed() < Duration::from_millis(100) {
                let ct = topic_ctr.load(Ordering::Relaxed);
                if ct == (i + 4) as usize {
                    good = true;
                    break;
                } else {
                    yield_now().await
                }
            }
            assert!(good);
        }

        // spawn topic handler
        for i in 0..3 {
            let mut msg = postcard::to_stdvec(&WireHeader {
                key: ZetaTopic3::TOPIC_KEY,
                seq_no: i,
            })
            .unwrap();
            let body = postcard::to_stdvec(&ZMsg(456)).unwrap();
            msg.extend_from_slice(&body);
            client_tx.send(msg).await.unwrap();

            let start = Instant::now();
            let mut good = false;
            while start.elapsed() < Duration::from_millis(100) {
                let ct = topic_ctr.load(Ordering::Relaxed);
                if ct == (i + 7) as usize {
                    good = true;
                    break;
                } else {
                    yield_now().await
                }
            }
            assert!(good);
        }
    }
}
