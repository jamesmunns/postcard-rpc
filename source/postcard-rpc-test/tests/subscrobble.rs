use core::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use std::sync::Arc;

use postcard_schema::Schema;
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, time::{sleep, timeout}};

use postcard_rpc::{
    define_dispatch, endpoints,
    header::{VarHeader, VarSeq, VarSeqKind},
    host_client::test_channels as client,
    server::{
        impls::test_channels::{
            dispatch_impl::{new_server, spawn_fn, Settings, WireSpawnImpl, WireTxImpl},
            ChannelWireRx, ChannelWireSpawn, ChannelWireTx,
        },
        Dispatch, Sender, SpawnContext,
    },
    topics,
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
#[derive(Serialize, Deserialize, Schema, PartialEq, Debug)]
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
async fn exclusive_subs_work() {
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
    let server_sender = server.sender();
    tokio::task::spawn(async move {
        server.run().await;
    });

    // Subbing works
    let cli = client::new_from_channels(client_tx, client_rx, VarSeqKind::Seq1);
    #[allow(deprecated)]
    let mut sub = cli.subscribe::<ZetaTopic10>(16).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(1), &ZMsg(10)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(2), &ZMsg(20)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(3), &ZMsg(30)).await.unwrap();
    let get_fut = async move {
        assert_eq!(sub.recv().await.unwrap(), ZMsg(10));
        assert_eq!(sub.recv().await.unwrap(), ZMsg(20));
        assert_eq!(sub.recv().await.unwrap(), ZMsg(30));
    };
    let _: () = timeout(Duration::from_millis(100), get_fut).await.unwrap();

    // Old subs are killed
    #[allow(deprecated)]
    let mut sub2 = cli.subscribe::<ZetaTopic10>(16).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(1), &ZMsg(11)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(2), &ZMsg(21)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(3), &ZMsg(31)).await.unwrap();
    // Ensure the sender has a chance to send the messages
    sleep(Duration::from_millis(10)).await;
    #[allow(deprecated)]
    let mut sub3 = cli.subscribe::<ZetaTopic10>(16).await.unwrap();
    sleep(Duration::from_millis(10)).await;
    let get_fut = async move {
        assert_eq!(sub2.recv().await.unwrap(), ZMsg(11));
        assert_eq!(sub2.recv().await.unwrap(), ZMsg(21));
        assert_eq!(sub2.recv().await.unwrap(), ZMsg(31));
        assert!(sub2.recv().await.is_none());
    };
    let _: () = timeout(Duration::from_millis(100), get_fut).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(1), &ZMsg(12)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(2), &ZMsg(22)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(3), &ZMsg(32)).await.unwrap();
    let get_fut = async move {
        assert_eq!(sub3.recv().await.unwrap(), ZMsg(12));
        assert_eq!(sub3.recv().await.unwrap(), ZMsg(22));
        assert_eq!(sub3.recv().await.unwrap(), ZMsg(32));
    };
    let _: () = timeout(Duration::from_millis(100), get_fut).await.unwrap();


    // Broadcast does not interfere
    #[allow(deprecated)]
    let mut sub4 = cli.subscribe::<ZetaTopic10>(16).await.unwrap();
    let mut sub5 = cli.subscribe_multi::<ZetaTopic10>(16).await.unwrap();
    sleep(Duration::from_millis(10)).await;
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(1), &ZMsg(15)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(2), &ZMsg(25)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(3), &ZMsg(35)).await.unwrap();
    // Ensure the sender has a chance to send the messages
    sleep(Duration::from_millis(10)).await;
    #[allow(deprecated)]
    let get_fut_excl = async move {
        assert_eq!(sub4.recv().await.unwrap(), ZMsg(15));
        assert_eq!(sub4.recv().await.unwrap(), ZMsg(25));
        assert_eq!(sub4.recv().await.unwrap(), ZMsg(35));
    };
    let get_fut_bcst = async move {
        assert_eq!(sub5.recv().await.unwrap(), ZMsg(15));
        assert_eq!(sub5.recv().await.unwrap(), ZMsg(25));
        assert_eq!(sub5.recv().await.unwrap(), ZMsg(35));
    };
    let _: () = timeout(Duration::from_millis(100), get_fut_excl).await.unwrap();
    let _: () = timeout(Duration::from_millis(100), get_fut_bcst).await.unwrap();
}

#[tokio::test]
async fn broadcast_subs_work() {
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
    let server_sender = server.sender();
    tokio::task::spawn(async move {
        server.run().await;
    });
    let cli = client::new_from_channels(client_tx, client_rx, VarSeqKind::Seq1);

    // Multi-Subbing works
    let mut sub1 = cli.subscribe_multi::<ZetaTopic10>(16).await.unwrap();
    let mut sub2 = cli.subscribe_multi::<ZetaTopic10>(16).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(1), &ZMsg(10)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(2), &ZMsg(20)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(3), &ZMsg(30)).await.unwrap();
    let get_fut1 = async move {
        assert_eq!(sub1.recv().await.unwrap(), ZMsg(10));
        assert_eq!(sub1.recv().await.unwrap(), ZMsg(20));
        assert_eq!(sub1.recv().await.unwrap(), ZMsg(30));
    };
    let get_fut2 = async move {
        assert_eq!(sub2.recv().await.unwrap(), ZMsg(10));
        assert_eq!(sub2.recv().await.unwrap(), ZMsg(20));
        assert_eq!(sub2.recv().await.unwrap(), ZMsg(30));
    };
    let _: () = timeout(Duration::from_millis(100), get_fut1).await.unwrap();
    let _: () = timeout(Duration::from_millis(100), get_fut2).await.unwrap();

    // Exclusive does not interfere
    let mut sub3 = cli.subscribe_multi::<ZetaTopic10>(16).await.unwrap();
    let mut sub4 = cli.subscribe_multi::<ZetaTopic10>(16).await.unwrap();
    #[allow(deprecated)]
    let mut sub5 = cli.subscribe::<ZetaTopic10>(16).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(1), &ZMsg(10)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(2), &ZMsg(20)).await.unwrap();
    server_sender.publish::<ZetaTopic10>(VarSeq::Seq4(3), &ZMsg(30)).await.unwrap();
    let get_fut1 = async move {
        assert_eq!(sub3.recv().await.unwrap(), ZMsg(10));
        assert_eq!(sub3.recv().await.unwrap(), ZMsg(20));
        assert_eq!(sub3.recv().await.unwrap(), ZMsg(30));
    };
    let get_fut2 = async move {
        assert_eq!(sub4.recv().await.unwrap(), ZMsg(10));
        assert_eq!(sub4.recv().await.unwrap(), ZMsg(20));
        assert_eq!(sub4.recv().await.unwrap(), ZMsg(30));
    };
    let get_fut3 = async move {
        assert_eq!(sub5.recv().await.unwrap(), ZMsg(10));
        assert_eq!(sub5.recv().await.unwrap(), ZMsg(20));
        assert_eq!(sub5.recv().await.unwrap(), ZMsg(30));
    };
    let _: () = timeout(Duration::from_millis(100), get_fut1).await.unwrap();
    let _: () = timeout(Duration::from_millis(100), get_fut2).await.unwrap();
    let _: () = timeout(Duration::from_millis(100), get_fut3).await.unwrap();

}
