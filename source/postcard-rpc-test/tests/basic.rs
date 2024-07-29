use std::{collections::HashMap, time::Duration};

use postcard::experimental::schema::Schema;
use postcard_rpc::test_utils::local_setup;
use postcard_rpc::{
    endpoint, headered::to_stdvec_keyed, topic, Dispatch, Endpoint, Key, Topic, WireHeader,
};
use serde::{Deserialize, Serialize};
use tokio::task::yield_now;
use tokio::time::timeout;

endpoint!(EndpointOne, Req1, Resp1, "endpoint/one");
topic!(TopicOne, Req1, "unsolicited/topic1");

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct Req1 {
    a: u8,
    b: u64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct Resp1 {
    c: [u8; 8],
    d: i32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub enum WireError {
    LeastBad,
    MediumBad,
    MostBad,
}

struct SmokeContext {
    got: HashMap<Key, (WireHeader, Vec<u8>)>,
    next_err: bool,
}

fn store_disp(hdr: &WireHeader, ctx: &mut SmokeContext, body: &[u8]) -> Result<(), WireError> {
    if ctx.next_err {
        ctx.next_err = false;
        return Err(WireError::MediumBad);
    }
    ctx.got.insert(hdr.key, (hdr.clone(), body.to_vec()));
    Ok(())
}

impl SmokeDispatch {
    pub fn new() -> Self {
        let ctx = SmokeContext {
            got: HashMap::new(),
            next_err: false,
        };
        let disp = Dispatch::new(ctx);
        Self { disp }
    }
}

struct SmokeDispatch {
    disp: Dispatch<SmokeContext, WireError, 8>,
}

#[tokio::test]
async fn smoke_reqresp() {
    let (mut srv, client) = local_setup::<WireError>(8, "error");

    // Create the Dispatch Server
    let mut disp = SmokeDispatch::new();
    disp.disp.add_handler::<EndpointOne>(store_disp).unwrap();

    // Start the request
    let send1 = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .send_resp::<EndpointOne>(&Req1 { a: 10, b: 100 })
                .await
        }
    });

    // As the wire, get the outgoing request
    let out1 = srv.recv_from_client().await.unwrap();

    // Does the outgoing value match what we expect?
    let exp_out = to_stdvec_keyed(0, EndpointOne::REQ_KEY, &Req1 { a: 10, b: 100 }).unwrap();
    let act_out = out1.to_bytes();
    assert_eq!(act_out, exp_out);

    // The request is still awaiting a response
    assert!(!send1.is_finished());

    // Feed the request through the dispatcher
    disp.disp.dispatch(&act_out).unwrap();

    // Make sure we "dispatched" it right
    let disp_got = disp.disp.context().got.remove(&out1.header.key).unwrap();
    assert_eq!(disp_got.0, out1.header);
    assert!(act_out.ends_with(&disp_got.1));

    // The request is still awaiting a response
    assert!(!send1.is_finished());

    // Feed a simulated response "from the wire" back to the
    // awaiting request
    const RESP_001: Resp1 = Resp1 {
        c: [1, 2, 3, 4, 5, 6, 7, 8],
        d: -10,
    };
    srv.reply::<EndpointOne>(out1.header.seq_no, &RESP_001)
        .await
        .unwrap();

    // Now wait for the request to complete
    let end = send1.await.unwrap().unwrap();

    // We got the simulated value back
    assert_eq!(end, RESP_001);
}

#[tokio::test]
async fn smoke_publish() {
    let (mut srv, client) = local_setup::<WireError>(8, "error");

    // Start the request
    client
        .publish::<TopicOne>(123, &Req1 { a: 10, b: 100 })
        .await
        .unwrap();

    // As the wire, get the outgoing request
    let out1 = srv.recv_from_client().await.unwrap();

    // Does the outgoing value match what we expect?
    let exp_out = to_stdvec_keyed(123, TopicOne::TOPIC_KEY, &Req1 { a: 10, b: 100 }).unwrap();
    let act_out = out1.to_bytes();
    assert_eq!(act_out, exp_out);
}

#[tokio::test]
async fn smoke_subscribe() {
    let (mut srv, client) = local_setup::<WireError>(8, "error");

    // Do a subscription
    let mut sub = client.subscribe::<TopicOne>(8).await.unwrap();

    // Start the listen
    let recv1 = timeout(Duration::from_millis(100), sub.recv());
    let _ = recv1.await.unwrap_err();

    // Send a message on the topic
    const VAL: Req1 = Req1 { a: 10, b: 100 };
    srv.publish::<TopicOne>(123, &VAL).await.unwrap();

    // Now the request resolves
    let publ = timeout(Duration::from_millis(100), sub.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(publ, VAL);
}

#[tokio::test]
async fn smoke_io_error() {
    let (mut srv, client) = local_setup::<WireError>(8, "error");

    // Do one round trip to make sure the connection works
    {
        let rr_rt = tokio::task::spawn({
            let client = client.clone();
            async move {
                client
                    .send_resp::<EndpointOne>(&Req1 { a: 10, b: 100 })
                    .await
            }
        });

        // As the wire, get the outgoing request
        let out1 = srv.recv_from_client().await.unwrap();

        // Does the outgoing value match what we expect?
        let exp_out = to_stdvec_keyed(0, EndpointOne::REQ_KEY, &Req1 { a: 10, b: 100 }).unwrap();
        let act_out = out1.to_bytes();
        assert_eq!(act_out, exp_out);

        // The request is still awaiting a response
        assert!(!rr_rt.is_finished());

        // Feed a simulated response "from the wire" back to the
        // awaiting request
        const RESP_001: Resp1 = Resp1 {
            c: [1, 2, 3, 4, 5, 6, 7, 8],
            d: -10,
        };
        srv.reply::<EndpointOne>(out1.header.seq_no, &RESP_001)
            .await
            .unwrap();

        // Now wait for the request to complete
        let end = rr_rt.await.unwrap().unwrap();

        // We got the simulated value back
        assert_eq!(end, RESP_001);
    }

    // Now, simulate an I/O error
    srv.cause_fatal_error();

    // Give the clients some time to halt
    yield_now().await;

    // Our server channels should now be closed - the tasks hung up
    assert!(srv.from_client.recv().await.is_none());
    assert!(srv.to_client.send(Vec::new()).await.is_err());

    // Try again, but nothing should work because all the worker tasks just halted
    {
        let rr_rt = tokio::task::spawn({
            let client = client.clone();
            async move {
                client
                    .send_resp::<EndpointOne>(&Req1 { a: 10, b: 100 })
                    .await
            }
        });

        // As the wire, get the outgoing request - didn't happen
        assert!(srv.recv_from_client().await.is_err());

        // Now wait for the request to complete - it failed
        rr_rt.await.unwrap().unwrap_err();
    }
}

#[tokio::test]
async fn smoke_closed() {
    let (mut srv, client) = local_setup::<WireError>(8, "error");

    // Do one round trip to make sure the connection works
    {
        let rr_rt = tokio::task::spawn({
            let client = client.clone();
            async move {
                client
                    .send_resp::<EndpointOne>(&Req1 { a: 10, b: 100 })
                    .await
            }
        });

        // As the wire, get the outgoing request
        let out1 = srv.recv_from_client().await.unwrap();

        // Does the outgoing value match what we expect?
        let exp_out = to_stdvec_keyed(0, EndpointOne::REQ_KEY, &Req1 { a: 10, b: 100 }).unwrap();
        let act_out = out1.to_bytes();
        assert_eq!(act_out, exp_out);

        // The request is still awaiting a response
        assert!(!rr_rt.is_finished());

        // Feed a simulated response "from the wire" back to the
        // awaiting request
        const RESP_001: Resp1 = Resp1 {
            c: [1, 2, 3, 4, 5, 6, 7, 8],
            d: -10,
        };
        srv.reply::<EndpointOne>(out1.header.seq_no, &RESP_001)
            .await
            .unwrap();

        // Now wait for the request to complete
        let end = rr_rt.await.unwrap().unwrap();

        // We got the simulated value back
        assert_eq!(end, RESP_001);
    }

    // Now, use the *client* to close the connection
    client.close();

    // Give the clients some time to halt
    yield_now().await;

    // Our server channels should now be closed - the tasks hung up
    assert!(srv.from_client.recv().await.is_none());
    assert!(srv.to_client.send(Vec::new()).await.is_err());

    // Try again, but nothing should work because all the worker tasks just halted
    {
        let rr_rt = tokio::task::spawn({
            let client = client.clone();
            async move {
                client
                    .send_resp::<EndpointOne>(&Req1 { a: 10, b: 100 })
                    .await
            }
        });

        // As the wire, get the outgoing request - didn't happen
        assert!(srv.recv_from_client().await.is_err());

        // Now wait for the request to complete - it failed
        rr_rt.await.unwrap().unwrap_err();
    }
}
