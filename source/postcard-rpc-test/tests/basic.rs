use core::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use std::{sync::Arc, time::Instant};

use postcard_schema::{schema::owned::OwnedNamedType, Schema};
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, task::yield_now};

use postcard_rpc::{
    define_dispatch, endpoints,
    header::{VarHeader, VarKey, VarKeyKind, VarSeq, VarSeqKind},
    host_client::test_channels as client,
    server::{
        impls::test_channels::{
            dispatch_impl::{new_server, spawn_fn, Settings, WireSpawnImpl, WireTxImpl},
            ChannelWireRx, ChannelWireSpawn, ChannelWireTx,
        },
        Dispatch, Sender, SpawnContext,
    },
    topics, Endpoint, Topic,
};

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

#[cfg(feature = "alpha")]
#[derive(Serialize, Deserialize, Schema)]
pub struct Message<'a> {
    data: &'a str,
}

#[cfg(not(feature = "alpha"))]
#[derive(Serialize, Deserialize, Schema)]
pub struct Message {
    data: String,
}

#[derive(Serialize, Deserialize, Schema)]
pub struct DoubleMessage<'a, 'b> {
    data1: &'a str,
    data2: &'b str,
}

endpoints! {
    list = ENDPOINT_LIST;
    | EndpointTy        | RequestTy             | ResponseTy            | Path              | Cfg                    |
    | ----------        | ---------             | ----------            | ----              | ---                    |
    | AlphaEndpoint     | AReq                  | AResp                 | "alpha"           |                        |
    | BetaEndpoint      | BReq                  | BResp                 | "beta"            |                        |
    | GammaEndpoint     | GReq                  | GResp                 | "gamma"           |                        |
    | DeltaEndpoint     | DReq                  | DResp                 | "delta"           |                        |
    | EpsilonEndpoint   | EReq                  | EResp                 | "epsilon"         |                        |
    | BorrowEndpoint1   | Message<'a>           | u8                    | "borrow1"         | cfg(feature = "alpha") |
    | BorrowEndpoint2   | ()                    | Message<'a>           | "borrow2"         |                        |
    | BorrowEndpoint3   | Message<'a>           | Message<'b>           | "borrow3"         |                        |
    | BorrowEndpoint4   | DoubleMessage<'a, 'b> | DoubleMessage<'c, 'd> | "borrow4"         |                        |
}

topics! {
    list = TOPICS_IN_LIST;
    direction = postcard_rpc::TopicDirection::ToServer;
    | TopicTy       | MessageTy             | Path      | Cfg                           |
    | ----------    | ---------             | ----      | ---                           |
    | ZetaTopic1    | ZMsg                  | "zeta1"   |                               |
    | ZetaTopic2    | ZMsg                  | "zeta2"   |                               |
    | ZetaTopic3    | ZMsg                  | "zeta3"   |                               |
    | BorrowTopic   | Message<'a>           | "msg1"    | cfg(feature = "alpha")        |
    | BorrowTopic   | DoubleMessage<'a, 'b> | "msg1"    | cfg(not(feature = "alpha"))   |
    | BerpTopic1    | u8                    | "empty"   |                               |
    | BerpTopic2    | ()                    | "empty"   |                               |
}

topics! {
    list = TOPICS_OUT_LIST;
    direction = postcard_rpc::TopicDirection::ToClient;
    | TopicTy           | MessageTy     | Path              |
    | ----------        | ---------     | ----              |
    | ZetaTopic10       | ZMsg          | "zeta10"          |
}

pub struct TestContext {
    pub ctr: Arc<AtomicUsize>,
    pub topic_ctr: Arc<AtomicUsize>,
    pub msg: String,
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

define_dispatch! {
    app: SingleDispatcher;
    spawn_fn: spawn_fn;
    tx_impl: WireTxImpl;
    spawn_impl: WireSpawnImpl;
    context: TestContext;

    endpoints: {
        list: ENDPOINT_LIST;

        | EndpointTy        | kind      | handler                   |
        | ----------        | ----      | -------                   |
        | AlphaEndpoint     | async     | test_alpha_handler        |
        | BetaEndpoint      | spawn     | test_beta_handler         |
        | BorrowEndpoint1   | blocking  | test_borrowep_blocking    |
        | BorrowEndpoint2   | blocking  | test_borrowep_blocking2   |
    };
    topics_in: {
        list: TOPICS_IN_LIST;

        | TopicTy           | kind      | handler               |
        | ----------        | ----      | -------               |
        | ZetaTopic1        | blocking  | test_zeta_blocking    |
        | ZetaTopic2        | async     | test_zeta_async       |
        | ZetaTopic3        | spawn     | test_zeta_spawn       |
        | BorrowTopic       | blocking  | test_borrow_blocking  |
    };
    topics_out: {
        list: TOPICS_OUT_LIST;
    };
}

fn test_borrowep_blocking2(
    context: &mut TestContext,
    _header: VarHeader,
    _body: (),
) -> Message<'_> {
    Message {
        data: context.msg.as_str(),
    }
}

fn test_borrowep_blocking(
    _context: &mut TestContext,
    _header: VarHeader,
    _body: Message<'_>,
) -> u8 {
    0
}

fn test_zeta_blocking(
    context: &mut TestContext,
    _header: VarHeader,
    _body: ZMsg,
    _out: &Sender<ChannelWireTx>,
) {
    context.topic_ctr.fetch_add(1, Ordering::Relaxed);
}

fn test_borrow_blocking(
    context: &mut TestContext,
    _header: VarHeader,
    _body: Message,
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

async fn test_alpha_handler(context: &mut TestContext, _header: VarHeader, body: AReq) -> AResp {
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
            msg: String::from("hello"),
        },
        ChannelWireSpawn {},
    );

    let cwrx = ChannelWireRx::new(server_rx);
    let cwtx = ChannelWireTx::new(server_tx);
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

#[tokio::test]
async fn end_to_end() {
    let (client_tx, server_rx) = mpsc::channel(16);
    let (server_tx, client_rx) = mpsc::channel(16);
    let topic_ctr = Arc::new(AtomicUsize::new(0));

    let app = SingleDispatcher::new(
        TestContext {
            ctr: Arc::new(AtomicUsize::new(0)),
            topic_ctr: topic_ctr.clone(),
            msg: String::from("hello"),
        },
        ChannelWireSpawn {},
    );

    let cwrx = ChannelWireRx::new(server_rx);
    let cwtx = ChannelWireTx::new(server_tx);

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
            msg: String::from("hello"),
        },
        ChannelWireSpawn {},
    );

    let cwrx = ChannelWireRx::new(server_rx);
    let cwtx = ChannelWireTx::new(server_tx);

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

#[test]
fn device_map() {
    let topic_ctr = Arc::new(AtomicUsize::new(0));
    let app = SingleDispatcher::new(
        TestContext {
            ctr: Arc::new(AtomicUsize::new(0)),
            topic_ctr: topic_ctr.clone(),
            msg: String::from("hello"),
        },
        ChannelWireSpawn {},
    );

    println!("# SingleDispatcher");
    println!();

    println!("## Types");
    println!();
    for ty in app.device_map.types {
        let ty = OwnedNamedType::from(*ty);
        println!("* {ty}");
    }

    println!();
    println!("## Endpoints");
    println!();
    for ep in app.device_map.endpoints {
        println!(
            "* {} ({:016X} -> {:016X})",
            ep.0,
            u64::from_le_bytes(ep.1.to_bytes()),
            u64::from_le_bytes(ep.2.to_bytes()),
        );
    }

    println!();
    println!("## Topics (In)");
    println!();
    for tp in app.device_map.topics_in {
        println!(
            "* {} <- ({:016X})",
            tp.0,
            u64::from_le_bytes(tp.1.to_bytes()),
        );
    }

    println!();
    println!("## Topics (Out)");
    println!();
    for tp in app.device_map.topics_out {
        println!(
            "* {} -> ({:016X})",
            tp.0,
            u64::from_le_bytes(tp.1.to_bytes()),
        );
    }
    println!();
    println!("## Min Key Length");
    println!();
    println!("{:?}", app.device_map.min_key_len);
    println!();
}
