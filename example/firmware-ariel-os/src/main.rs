#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(used_with_arg)]

use ariel_os::{
    asynch::Spawner,
    debug::log::info,
    reexports::embassy_usb,
    time::{Duration, Timer},
    usb,
};

use postcard_rpc::{
    define_dispatch,
    header::VarHeader,
    server::{
        impls::embassy_usb_v0_4::{
            dispatch_impl::{WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorageNoUsb, WireTxImpl},
            PacketBuffers,
        },
        Dispatch, Server,
    },
};
use static_cell::ConstStaticCell;
use workbook_icd::{PingEndpoint, ENDPOINT_LIST, TOPICS_IN_LIST, TOPICS_OUT_LIST};

pub struct Context;

type AppMutex = embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
type AppDriver = usb::UsbDriver;
type AppStorage = WireStorageNoUsb<AppMutex, AppDriver>;
type BufStorage = PacketBuffers<1024, 1024>;
type AppTx = WireTxImpl<AppMutex, AppDriver>;
type AppRx = WireRxImpl<AppDriver>;
type AppServer = Server<AppTx, AppRx, WireRxBuf, MyApp>;

static PBUFS: ConstStaticCell<BufStorage> = ConstStaticCell::new(BufStorage::new());
static STORAGE: AppStorage = AppStorage::new();

/// Helper to get unique ID from flash
pub fn get_unique_id() -> Option<u64> {
    // TODO
    Some(12345678)
}

#[ariel_os::config(usb)]
const USB_CONFIG: embassy_usb::Config = {
    let mut config = embassy_usb::Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("OneVariable");
    config.product = Some("ov-twin");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config
};

define_dispatch! {
    app: MyApp;
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

#[ariel_os::task(autostart, usb_builder_hook)]
async fn main() {
    info!("Start");

    let unique_id = get_unique_id().unwrap();
    info!("id: {=u64:016X}", unique_id);

    // USB/RPC INIT
    let pbufs = PBUFS.take();
    let context = Context;

    // Create and inject the Postcard usb endpoint on the system USB builder.
    let (tx_impl, rx_impl) = USB_BUILDER_HOOK
        .with(|builder| STORAGE.init(builder, pbufs.tx_buf.as_mut_slice()))
        .await;

    let spawner = Spawner::for_current_executor().await;
    let dispatcher = MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let mut server: AppServer = Server::new(
        tx_impl,
        rx_impl,
        pbufs.rx_buf.as_mut_slice(),
        dispatcher,
        vkk,
    );

    loop {
        // Somehow at least on nrf52840dk, this is needed, otherwise the
        // `ariel_os_embassy::init_task()` task starves.
        Timer::after(Duration::from_millis(100)).await;

        // If the host disconnects, we'll return an error here.
        // If this happens, just wait until the host reconnects
        let _ = server.run().await;
    }
}

// ---

fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping");
    rqst
}
