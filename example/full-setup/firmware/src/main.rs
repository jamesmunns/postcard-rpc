#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use comms::Dispatcher;
// use crate::{
//     comms::init_sender,
//     usb::{configure_usb, usb_task, UsbResources},
// };
use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts, peripherals::USB_OTG_FS, rcc::{
        AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPreDiv, PllSource,
        Sysclk,
    }, time::Hertz, usb::{Driver, Endpoint, Out}, Config
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use postcard_rpc::target_server::{configure_usb, example_config, rpc_dispatch, Sender, SenderInner, UsbBuffers};
use static_cell::StaticCell;

use crate::usb::usb_task;

use {defmt_rtt as _, panic_probe as _};

static EP_OUT_BUF: StaticCell<[u8; 256]> = StaticCell::new();
static TX_BUF: StaticCell<[u8; 256]> = StaticCell::new();
static OTHER_BUFS: StaticCell<UsbBuffers> = StaticCell::new();
static SENDER_INNER: StaticCell<Mutex<ThreadModeRawMutex, SenderInner<Driver<'static, USB_OTG_FS>>>> = StaticCell::new();

mod comms;
mod usb;

bind_interrupts!(pub struct Irqs {
    OTG_FS => embassy_stm32::usb::InterruptHandler<embassy_stm32::peripherals::USB_OTG_FS>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    let config = {
        let mut config = Config::default();
        config.rcc.hse = Some(Hse {
            freq: Hertz(12_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV6,
            mul: PllMul::MUL168,
            divp: Some(embassy_stm32::rcc::PllPDiv::DIV2), // 12mhz / 6 * 168 / 2 = 168Mhz.
            divq: Some(embassy_stm32::rcc::PllQDiv::DIV7), // 12mhz / 6 * 168 / 7 = 48Mhz.
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
        config
    };

    let p = embassy_stm32::init(config);
    let ep_out_buffer = EP_OUT_BUF.init([0u8; 256]);
    let bufs = OTHER_BUFS.init(UsbBuffers::new());
    let tx_buf = TX_BUF.init([0u8; 256]);

    // Create the driver, from the HAL.
    let mut config = embassy_stm32::usb::Config::default();
    config.vbus_detection = false;
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, config);
    let usb_config = example_config();

    let (d, ep_in, ep_out) = configure_usb(driver, bufs, usb_config);
    let sender = Sender::init_sender(&SENDER_INNER, tx_buf, ep_in);
    let dispatch = Dispatcher::new();

    spawner.must_spawn(usb_task(d));
    spawner.must_spawn(dispatch_task(ep_out, sender, dispatch));
}

#[embassy_executor::task]
async fn dispatch_task(
    ep_out: Endpoint<'static, USB_OTG_FS, Out>,
    sender: Sender<ThreadModeRawMutex, Driver<'static, USB_OTG_FS>>,
    dispatch: Dispatcher,
) {
    let mut rx_buf = [0u8; 256];
    rpc_dispatch(ep_out, sender, dispatch, &mut rx_buf).await;
}
