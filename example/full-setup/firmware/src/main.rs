#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    peripherals::{self, USB_OTG_FS},
    rcc::{AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPreDiv, PllSource, Sysclk},
    time::Hertz,
    usb::{Driver, Endpoint, Out},
    Config,
};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_time::{Duration, Timer};
use embassy_usb::UsbDevice;
use james_icd::sleep::{Sleep, SleepDone, SleepEndpoint};
use postcard_rpc::{
    define_dispatch,
    target_server::{buffers::AllBuffers, configure_usb, example_config, rpc_dispatch, Sender},
    WireHeader,
};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// These are all buffers needed by postcard-rpc's server
static ALL_BUFFERS: StaticCell<AllBuffers<256, 256, 256>> = StaticCell::new();

bind_interrupts!(pub struct Irqs {
    OTG_FS => embassy_stm32::usb::InterruptHandler<embassy_stm32::peripherals::USB_OTG_FS>;
});

define_dispatch! {
    dispatcher: Dispatcher<Mutex = ThreadModeRawMutex, Driver = Driver<'static, USB_OTG_FS>>;
    SleepEndpoint => spawn sleep_task,
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    // This is the configuration for the Adafruit STM32F405 Feather Express.
    // Your config may be different!
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
    let bufs = ALL_BUFFERS.init(AllBuffers::new());

    // Create the driver, from the HAL.
    let mut config = embassy_stm32::usb::Config::default();
    config.vbus_detection = false;
    let driver = Driver::new_fs(
        p.USB_OTG_FS,
        Irqs,
        p.PA12,
        p.PA11,
        &mut bufs.endpoint_out,
        config,
    );
    let usb_config = example_config();
    let (d, ep_in, ep_out) = configure_usb(driver, &mut bufs.usb_device, usb_config);

    let dispatch = Dispatcher::new(&mut bufs.tx_buf, ep_in);

    spawner.must_spawn(usb_task(d));
    spawner.must_spawn(dispatch_task(ep_out, dispatch, &mut bufs.rx_buf));
}

/// This actually runs the dispatcher
#[embassy_executor::task]
async fn dispatch_task(
    ep_out: Endpoint<'static, USB_OTG_FS, Out>,
    dispatch: Dispatcher,
    rx_buf: &'static mut [u8],
) {
    rpc_dispatch(ep_out, dispatch, rx_buf).await;
}

/// This handles the low level USB management
#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

/// And this is our example RPC task!
#[embassy_executor::task(pool_size = 3)]
async fn sleep_task(
    header: WireHeader,
    s: Sleep,
    sender: Sender<ThreadModeRawMutex, Driver<'static, USB_OTG_FS>>,
) {
    info!("Sleep spawned");
    Timer::after(Duration::from_secs(s.seconds.into())).await;
    Timer::after(Duration::from_micros(s.micros.into())).await;
    info!("Sleep complete");
    let msg = SleepDone { slept_for: s };
    let _ = sender.reply::<SleepEndpoint>(header.seq_no, &msg).await;
}
