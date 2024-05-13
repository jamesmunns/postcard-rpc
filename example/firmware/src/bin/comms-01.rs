#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::{
    peripherals::USB,
    usb::{self, Driver, Endpoint, Out},
};

use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

use embassy_usb::UsbDevice;
use postcard_rpc::{
    define_dispatch,
    target_server::{buffers::AllBuffers, configure_usb, example_config, rpc_dispatch},
    WireHeader,
};

use static_cell::ConstInitCell;
use workbook_fw::{get_unique_id, Irqs};
use workbook_icd::PingEndpoint;

static ALL_BUFFERS: ConstInitCell<AllBuffers<256, 256, 256>> =
    ConstInitCell::new(AllBuffers::new());

pub struct Context {}

define_dispatch! {
    dispatcher: Dispatcher<
        Mutex = ThreadModeRawMutex,
        Driver = usb::Driver<'static, USB>,
        Context = Context
    >;
    PingEndpoint => blocking ping_handler,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // SYSTEM INIT
    info!("Start");

    let mut p = embassy_rp::init(Default::default());
    let unique_id = get_unique_id(&mut p.FLASH).unwrap();
    info!("id: {=u64:016X}", unique_id);

    // USB/RPC INIT
    let driver = usb::Driver::new(p.USB, Irqs);
    let mut config = example_config();
    config.manufacturer = Some("OneVariable");
    config.product = Some("ov-twin");
    let buffers = ALL_BUFFERS.take();
    let (device, ep_in, ep_out) = configure_usb(driver, &mut buffers.usb_device, config);
    let dispatch = Dispatcher::new(&mut buffers.tx_buf, ep_in, Context {});

    spawner.must_spawn(dispatch_task(ep_out, dispatch, &mut buffers.rx_buf));
    spawner.must_spawn(usb_task(device));
}

/// This actually runs the dispatcher
#[embassy_executor::task]
async fn dispatch_task(
    ep_out: Endpoint<'static, USB, Out>,
    dispatch: Dispatcher,
    rx_buf: &'static mut [u8],
) {
    rpc_dispatch(ep_out, dispatch, rx_buf).await;
}

/// This handles the low level USB management
#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) {
    usb.run().await;
}

fn ping_handler(_context: &mut Context, header: WireHeader, rqst: u32) -> u32 {
    info!("ping: seq - {=u32}", header.seq_no);
    rqst
}
