use std::collections::HashMap;

use postcard::experimental::schema::Schema;
use postcard_rpc::{
    endpoint,
    headered::to_stdvec_keyed,
    host_client::{HostClient, RpcFrame},
    Dispatch, Endpoint, Key, WireHeader,
};
use serde::{Deserialize, Serialize};

endpoint!(EndpointOne, Req1, Resp1, "endpoint/one");

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
async fn smoke() {
    // Create the Host Client
    let (client, mut wire) = HostClient::<WireError>::new_manual("error", 8);

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
    let out1 = wire.outgoing.recv().await.unwrap();

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
    wire.incoming
        .process(RpcFrame {
            header: WireHeader {
                key: EndpointOne::RESP_KEY,
                seq_no: 0,
            },
            body: postcard::to_stdvec(&Resp1 {
                c: [1, 2, 3, 4, 5, 6, 7, 8],
                d: -10,
            })
            .unwrap(),
        })
        .unwrap();

    // Now wait for the request to complete
    let end = send1.await.unwrap().unwrap();

    // We got the simulated value back
    assert_eq!(
        end,
        Resp1 {
            c: [1, 2, 3, 4, 5, 6, 7, 8],
            d: -10
        }
    );
}
