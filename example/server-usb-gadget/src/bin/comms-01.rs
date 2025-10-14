use postcard_rpc::{
    define_dispatch,
    header::VarHeader,
    server::{
        impls::usb_gadget::dispatch_impl::{
            WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorage, WireTxImpl,
        },
        Dispatch, Server,
    },
};
use static_cell::StaticCell;
use workbook_icd::{PingEndpoint, ENDPOINT_LIST, TOPICS_IN_LIST, TOPICS_OUT_LIST};

pub struct Context;

type AppTx = WireTxImpl;
type AppRx = WireRxImpl;
type AppServer = Server<AppTx, AppRx, WireRxBuf, Dispatcher>;

static STORAGE: WireStorage = WireStorage::new();

define_dispatch! {
    app: Dispatcher;
    spawn_fn: spawn_fn;
    tx_impl: AppTx;
    spawn_impl: WireSpawnImpl;
    context: Context;

    endpoints: {
        list: ENDPOINT_LIST;

        | EndpointTy                | kind      | handler                       |
        | ----------                | ----      | -------                       |
        | PingEndpoint              | blocking  | ping_handler                  |
    };
    topics_in: {
        list: TOPICS_IN_LIST;

        | TopicTy                   | kind      | handler                       |
        | ----------                | ----      | -------                       |
    };
    topics_out: {
        list: TOPICS_OUT_LIST;
    };
}

static RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();

use usb_gadget::{function::custom::Event, Class, Gadget, Id, Strings};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    usb_gadget::remove_all().expect("cannot remove all gadgets");

    let gadget = Gadget::new(
        Class::new(255, 255, 3),
        Id::new(0xdead, 0xbeef),
        Strings::new("manufacturer", "custom USB interface", "serial_number"),
    );

    let context = Context;
    let ((reg, mut custom), tx_impl, rx_impl) = STORAGE.init(gadget);
    let dispatcher = Dispatcher::new(context, tokio::runtime::Handle::current().into());

    tokio::spawn(async move {
        let mut ctrl_data = Vec::new();

        custom.wait_event().await.expect("wait for event failed");
        println!("event ready");
        let event = custom.event().expect("event failed");

        println!("Event: {event:?}");
        match event {
            Event::SetupHostToDevice(req) => {
                if req.ctrl_req().request == 255 {
                    println!("Stopping");
                    todo!("STOP");
                }
                ctrl_data = req.recv_all().unwrap();
                println!("Control data: {ctrl_data:x?}");
            }
            Event::SetupDeviceToHost(req) => {
                println!("Replying with data");
                req.send(&ctrl_data).unwrap();
            }
            _ => (),
        }
    });

    let rx_buf = RX_BUF.init([0u8; 1024]);

    let vkk = dispatcher.min_key_len();
    let mut server: AppServer =
        Server::new(tx_impl, rx_impl, rx_buf.as_mut_slice(), dispatcher, vkk);

    loop {
        // If the host disconnects, we'll return an error here.
        // If this happens, just wait until the host reconnects
        let _ = server.run().await;
    }
}

// ---

fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    println!("ping");
    rqst
}
