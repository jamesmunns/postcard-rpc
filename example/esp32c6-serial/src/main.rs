#![no_std]
#![no_main]

use embassy_executor::Spawner;
use esp_hal::{
    clock::CpuClock, efuse, rmt::Rmt, time::Rate, timer::systimer::SystemTimer,
    usb_serial_jtag::UsbSerialJtag,
};
use esp_hal_smartled::SmartLedsAdapter;
use panic_rtt_target as _;
use postcard_rpc::server::{Dispatch, Server};
use static_cell::ConstStaticCell;

use crate::app::{AppServer, Context, MyApp, STORAGE};

pub mod app;
pub mod handlers;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    rtt_target::rtt_init_defmt!();

    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let timer0 = SystemTimer::new(p.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    let (rx, tx) = UsbSerialJtag::new(p.USB_DEVICE).into_async().split();
    let (rx_impl, tx_impl) = STORAGE.init(rx, tx).unwrap();

    let rmt = Rmt::new(p.RMT, Rate::from_mhz(80)).unwrap();

    let context = Context {
        unique_id: get_unique_id(),
        led: SmartLedsAdapter::new(rmt.channel0, p.GPIO8, <_>::default()),
        leds: <_>::default(),
    };

    static PACKET_RX_BUF: ConstStaticCell<[u8; 1024]> = ConstStaticCell::new([0u8; 1024]);

    let dispatcher = MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let mut server: AppServer =
        Server::new(tx_impl, rx_impl, PACKET_RX_BUF.take(), dispatcher, vkk);

    loop {
        let _ = server.run().await;
    }
}

fn get_unique_id() -> u64 {
    let mac = efuse::Efuse::mac_address();
    u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0, 0])
}
