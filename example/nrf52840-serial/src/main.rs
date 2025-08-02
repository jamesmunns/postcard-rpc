#![no_std]
#![no_main]

use app::{AppServer, STORAGE};
use defmt::info;
use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts,
    buffered_uarte::{self, BufferedUarte},
    config::{Config as NrfConfig, HfclkSource},
    gpio::{Level, Output, OutputDrive},
    pac::FICR,
    peripherals::UARTE0,
    uarte,
};
use embassy_time::Timer;
use postcard_rpc::server::{Dispatch, Server};
use static_cell::ConstStaticCell;

bind_interrupts!(pub struct Irqs {
    UARTE0 => buffered_uarte::InterruptHandler<UARTE0>;
});

use {defmt_rtt as _, panic_probe as _};

pub mod app;
pub mod handlers;
pub mod icd;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // SYSTEM INIT
    info!("Start");
    let mut config = NrfConfig::default();
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(Default::default());
    // Obtain the device ID
    let unique_id = get_unique_id();

    defmt::println!("Hello");

    let mut config = uarte::Config::default();
    config.parity = uarte::Parity::EXCLUDED;
    config.baudrate = uarte::Baudrate::BAUD115200;

    static SERIAL_TX_BUF: ConstStaticCell<[u8; 1024]> = ConstStaticCell::new([0u8; 1024]);
    static SERIAL_RX_BUF: ConstStaticCell<[u8; 1024]> = ConstStaticCell::new([0u8; 1024]);
    static PACKET_RX_BUF: ConstStaticCell<[u8; 1024]> = ConstStaticCell::new([0u8; 1024]);

    let u = BufferedUarte::new(
        p.UARTE0,
        p.TIMER0,
        p.PPI_CH0,
        p.PPI_CH1,
        p.PPI_GROUP0,
        p.P0_08,
        p.P0_06,
        Irqs,
        config,
        SERIAL_RX_BUF.take(),
        SERIAL_TX_BUF.take(),
    );

    let (erx, etx) = u.split();
    let (rx_impl, tx_impl) = STORAGE.init(erx, etx).unwrap();
    let led = Output::new(p.P0_13, Level::Low, OutputDrive::Standard);

    let context = app::Context { unique_id, led };

    let dispatcher = app::MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let server: app::AppServer =
        Server::new(tx_impl, rx_impl, PACKET_RX_BUF.take(), dispatcher, vkk);
    spawner.must_spawn(run_server(server));
}

#[embassy_executor::task]
async fn run_server(mut server: AppServer) {
    // Begin running!
    loop {
        // If the host disconnects, we'll return an error here.
        // If this happens, just wait until the host reconnects
        let _ = server.run().await;
        defmt::info!("I/O error");
        Timer::after_millis(100).await;
    }
}

fn get_unique_id() -> u64 {
    let lower = FICR.deviceid(0).read() as u64;
    let upper = FICR.deviceid(1).read() as u64;
    (upper << 32) | lower
}
