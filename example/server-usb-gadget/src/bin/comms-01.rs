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
use usb_gadget::{Class, Gadget, Id, Strings};
use workbook_icd::{PingEndpoint, ENDPOINT_LIST, TOPICS_IN_LIST, TOPICS_OUT_LIST};

pub struct Context;

type AppTx = WireTxImpl;
type AppRx = WireRxImpl;
type AppServer = Server<AppTx, AppRx, WireRxBuf, Dispatcher>;

static RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();
static TX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();
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

#[tokio::main(flavor = "current_thread")]
async fn main() {
    usb_gadget::remove_all().expect("cannot remove all gadgets");

    let gadget = Gadget::new(
        Class::new(0xEF, 0x02, 0x01),
        Id::new(0xdead, 0xbeef),
        Strings::new("manufacturer", "ov-twin", "serial_number"),
    );

    let context = Context;
    let rx_buf = RX_BUF.init([0u8; 1024]);
    let tx_buf = TX_BUF.init([0u8; 1024]);

    let (_reg, tx_impl, rx_impl) = STORAGE
        .init(gadget, tx_buf.as_mut_slice())
        .expect("Failed to init");
    let dispatcher = Dispatcher::new(context, tokio::runtime::Handle::current().into());

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
