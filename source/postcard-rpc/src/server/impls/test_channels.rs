//! Implementation that uses channels for local testing

use core::{convert::Infallible, future::Future};

use crate::server::{
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
        pub kkind: VarKeyKind,
    }

    pub type WireTxImpl = super::ChannelWireTx;
    pub type WireRxImpl = super::ChannelWireRx;
    pub type WireSpawnImpl = super::ChannelWireSpawn;
    pub type WireRxBuf = Box<[u8]>;

    use crate::{
        header::VarKeyKind,
        server::{Dispatch2, Server},
    };

    pub use super::tokio_spawn as spawn_fn;

    pub fn new_server<D>(
        dispatch: D,
        settings: Settings,
    ) -> crate::server::Server<WireTxImpl, WireRxImpl, WireRxBuf, D>
    where
        D: Dispatch2<Tx = WireTxImpl>,
    {
        let buf = vec![0; settings.buf];
        Server::new(
            &settings.tx,
            settings.rx,
            buf.into_boxed_slice(),
            dispatch,
            settings.kkind,
        )
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
        hdr: crate::header::VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut hdr_ser = hdr.write_to_vec();
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
        define_dispatch, endpoints,
        header::{VarHeader, VarKey, VarKeyKind, VarSeq, VarSeqKind},
        server::{Dispatch2, Sender, SpawnContext},
        topics, Endpoint, Topic,
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
    use crate::server::impls::test_channels::dispatch_impl::{
        new_server, spawn_fn, Settings, WireSpawnImpl, WireTxImpl,
    };

    define_dispatch! {
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
        _header: VarHeader,
        _body: ZMsg,
        _out: &Sender<ChannelWireTx>,
    ) {
        context.topic_ctr.fetch_add(1, Ordering::Relaxed);
    }

    async fn test_zeta_async(
        context: &mut TestContext,
        _header: VarHeader,
        _body: ZMsg,
        _out: &Sender<ChannelWireTx>,
    ) {
        context.topic_ctr.fetch_add(1, Ordering::Relaxed);
    }

    async fn test_zeta_spawn(
        context: TestSpawnContext,
        _header: VarHeader,
        _body: ZMsg,
        _out: Sender<ChannelWireTx>,
    ) {
        context.topic_ctr.fetch_add(1, Ordering::Relaxed);
    }

    async fn test_alpha_handler(
        context: &mut TestContext,
        _header: VarHeader,
        body: AReq,
    ) -> AResp {
        context.ctr.fetch_add(1, Ordering::Relaxed);
        AResp(body.0)
    }

    async fn test_beta_handler(
        context: TestSpawnContext,
        header: VarHeader,
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
        let topic_ctr = Arc::new(AtomicUsize::new(0));

        let app = SingleDispatcher::new(
            TestContext {
                ctr: Arc::new(AtomicUsize::new(0)),
                topic_ctr: topic_ctr.clone(),
            },
            ChannelWireSpawn {},
        );

        let cwrx = ChannelWireRx { rx: server_rx };
        let cwtx = ChannelWireTx { tx: server_tx };
        let kkind = app.min_key_len();
        let mut server = new_server(
            app,
            Settings {
                tx: cwtx,
                rx: cwrx,
                buf: 1024,
                kkind,
            },
        );
        tokio::task::spawn(async move {
            server.run().await;
        });

        // manually build request - Alpha
        let mut msg = VarHeader {
            key: VarKey::Key8(AlphaEndpoint::REQ_KEY),
            seq_no: VarSeq::Seq4(123),
        }
        .write_to_vec();
        let body = postcard::to_stdvec(&AReq(42)).unwrap();
        msg.extend_from_slice(&body);
        client_tx.send(msg).await.unwrap();
        let resp = client_rx.recv().await.unwrap();

        // manually extract response
        let (hdr, body) = VarHeader::take_from_slice(&resp).unwrap();
        let resp = postcard::from_bytes::<<AlphaEndpoint as Endpoint>::Response>(body).unwrap();
        assert_eq!(resp.0, 42);
        assert_eq!(hdr.key, VarKey::Key8(AlphaEndpoint::RESP_KEY));
        assert_eq!(hdr.seq_no, VarSeq::Seq4(123));

        // manually build request - Beta
        let mut msg = VarHeader {
            key: VarKey::Key8(BetaEndpoint::REQ_KEY),
            seq_no: VarSeq::Seq4(234),
        }
        .write_to_vec();
        let body = postcard::to_stdvec(&BReq(1000)).unwrap();
        msg.extend_from_slice(&body);
        client_tx.send(msg).await.unwrap();
        let resp = client_rx.recv().await.unwrap();

        // manually extract response
        let (hdr, body) = VarHeader::take_from_slice(&resp).unwrap();
        let resp = postcard::from_bytes::<<BetaEndpoint as Endpoint>::Response>(body).unwrap();
        assert_eq!(resp.0, 1000);
        assert_eq!(hdr.key, VarKey::Key8(BetaEndpoint::RESP_KEY));
        assert_eq!(hdr.seq_no, VarSeq::Seq4(234));

        // blocking topic handler
        for i in 0..3 {
            let mut msg = VarHeader {
                key: VarKey::Key8(ZetaTopic1::TOPIC_KEY),
                seq_no: VarSeq::Seq4(i),
            }
            .write_to_vec();

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
            let mut msg = VarHeader {
                key: VarKey::Key8(ZetaTopic2::TOPIC_KEY),
                seq_no: VarSeq::Seq4(i),
            }
            .write_to_vec();
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
            let mut msg = VarHeader {
                key: VarKey::Key8(ZetaTopic3::TOPIC_KEY),
                seq_no: VarSeq::Seq4(i),
            }
            .write_to_vec();
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

    use crate::host_client::test_channels as client;
    #[tokio::test]
    async fn end_to_end() {
        let (client_tx, server_rx) = mpsc::channel(16);
        let (server_tx, client_rx) = mpsc::channel(16);
        let topic_ctr = Arc::new(AtomicUsize::new(0));

        let app = SingleDispatcher::new(
            TestContext {
                ctr: Arc::new(AtomicUsize::new(0)),
                topic_ctr: topic_ctr.clone(),
            },
            ChannelWireSpawn {},
        );

        let cwrx = ChannelWireRx { rx: server_rx };
        let cwtx = ChannelWireTx { tx: server_tx };

        let kkind = app.min_key_len();
        let mut server = new_server(
            app,
            Settings {
                tx: cwtx,
                rx: cwrx,
                buf: 1024,
                kkind,
            },
        );
        tokio::task::spawn(async move {
            server.run().await;
        });

        let cli = client::new_from_channels(client_tx, client_rx, VarSeqKind::Seq1);

        let resp = cli.send_resp::<AlphaEndpoint>(&AReq(42)).await.unwrap();
        assert_eq!(resp.0, 42);
        let resp = cli.send_resp::<BetaEndpoint>(&BReq(1234)).await.unwrap();
        assert_eq!(resp.0, 1234);
    }

    #[tokio::test]
    async fn end_to_end_force8() {
        let (client_tx, server_rx) = mpsc::channel(16);
        let (server_tx, client_rx) = mpsc::channel(16);
        let topic_ctr = Arc::new(AtomicUsize::new(0));

        let app = SingleDispatcher::new(
            TestContext {
                ctr: Arc::new(AtomicUsize::new(0)),
                topic_ctr: topic_ctr.clone(),
            },
            ChannelWireSpawn {},
        );

        let cwrx = ChannelWireRx { rx: server_rx };
        let cwtx = ChannelWireTx { tx: server_tx };

        let kkind = VarKeyKind::Key8;
        let mut server = new_server(
            app,
            Settings {
                tx: cwtx,
                rx: cwrx,
                buf: 1024,
                kkind,
            },
        );
        tokio::task::spawn(async move {
            server.run().await;
        });

        let cli = client::new_from_channels(client_tx, client_rx, VarSeqKind::Seq4);

        let resp = cli.send_resp::<AlphaEndpoint>(&AReq(42)).await.unwrap();
        assert_eq!(resp.0, 42);
        let resp = cli.send_resp::<BetaEndpoint>(&BReq(1234)).await.unwrap();
        assert_eq!(resp.0, 1234);
    }
}