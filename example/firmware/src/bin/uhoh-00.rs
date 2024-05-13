//! This is what will be flashed to all boards before the workshop starts.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::{peripherals::PIO0, pio::Pio};

use embassy_time::{Duration, Ticker};

use smart_leds::RGB;
use workbook_fw::{
    get_unique_id,
    ws2812::{self, Ws2812},
    NUM_SMARTLEDS,
};

// GPIO pins we'll need for this part:
//
// | GPIO Name | Usage                     | Notes                             |
// | :---      | :---                      | :---                              |
// | GPIO25    | Smart LED                 | 3v3 output                        |

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // SYSTEM INIT
    info!("Start");

    let mut p = embassy_rp::init(Default::default());
    let unique_id = get_unique_id(&mut p.FLASH).unwrap();
    info!("id: {=u64:016X}", unique_id);

    // PIO/WS2812 INIT
    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, ws2812::Irqs);

    // GPIO25 is used for
    let ws2812: Ws2812<'static, PIO0, 0, NUM_SMARTLEDS> =
        Ws2812::new(&mut common, sm0, p.DMA_CH0, p.PIN_25);

    // Start the
    spawner.must_spawn(led_task(ws2812));
}

// This is our LED task
#[embassy_executor::task]
async fn led_task(mut ws2812: Ws2812<'static, PIO0, 0, NUM_SMARTLEDS>) {
    let mut ticker = Ticker::every(Duration::from_millis(25));
    // Fade red up and down so I can see who hasn't been able to flash their board yet
    loop {
        // Up
        for i in 0..=32 {
            ticker.next().await;
            let color = RGB { r: i, g: 0, b: 0 };
            let colors = [color; NUM_SMARTLEDS];
            ws2812.write(&colors).await;
        }

        // Down
        for i in (0..=32).rev() {
            ticker.next().await;
            let color = RGB { r: i, g: 0, b: 0 };
            let colors = [color; NUM_SMARTLEDS];
            ws2812.write(&colors).await;
        }

        // Wait
        for _ in 0..=32 {
            ticker.next().await;
        }
    }
}
