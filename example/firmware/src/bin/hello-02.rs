#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::{peripherals::PIO0, pio::Pio};

use embassy_time::{Duration, Ticker};

use smart_leds::colors;
use workbook_fw::{
    get_unique_id,
    ws2812::{self, Ws2812},
    Buttons, Potentiometer, NUM_SMARTLEDS,
};

// GPIO pins we'll need for this part:
//
// | GPIO Name | Usage                     | Notes                             |
// | :---      | :---                      | :---                              |
// | GPIO00    | Button 1                  | Button Pad (left) - active LOW    |
// | GPIO01    | Button 2                  | Button Pad (left) - active LOW    |
// | GPIO02    | Button 3                  | Button Pad (left) - active LOW    |
// | GPIO03    | Button 4                  | Button Pad (left) - active LOW    |
// | GPIO18    | Button 5                  | Button Pad (right) - active LOW   |
// | GPIO19    | Button 6                  | Button Pad (right) - active LOW   |
// | GPIO20    | Button 7                  | Button Pad (right) - active LOW   |
// | GPIO21    | Button 8                  | Button Pad (right) - active LOW   |
// | GPIO25    | Smart LED                 | 3v3 output                        |
// | GPIO26    | ADC0                      | Potentiometer                     |
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

    // GPIO25 is used for Smart LEDs
    let ws2812: Ws2812<'static, PIO0, 0, NUM_SMARTLEDS> =
        Ws2812::new(&mut common, sm0, p.DMA_CH0, p.PIN_25);

    let buttons = Buttons::new(
        p.PIN_0, p.PIN_1, p.PIN_2, p.PIN_3, p.PIN_18, p.PIN_19, p.PIN_20, p.PIN_21,
    );
    let potentiometer = Potentiometer::new(p.ADC, p.PIN_26);

    // Start the LED task
    spawner.must_spawn(led_task(ws2812));

    // Start the Button task
    spawner.must_spawn(button_task(buttons));

    // Start the Potentiometer task
    spawner.must_spawn(pot_task(potentiometer));
}

// This is our Button task
#[embassy_executor::task]
async fn button_task(buttons: Buttons) {
    let mut last = [false; Buttons::COUNT];
    let mut ticker = Ticker::every(Duration::from_millis(10));
    loop {
        ticker.next().await;
        let now = buttons.read_all();
        if now != last {
            info!("Buttons changed: {:?}", now);
            last = now;
        }
    }
}

// This is our Potentiometer task
#[embassy_executor::task]
async fn pot_task(mut pot: Potentiometer) {
    let mut last = pot.read().await;
    let mut ticker = Ticker::every(Duration::from_millis(100));
    loop {
        ticker.next().await;
        let now = pot.read().await;
        if now.abs_diff(last) > 64 {
            info!("Potentiometer changed: {=u16}", now);
            last = now;
        }
    }
}

// This is our LED task
#[embassy_executor::task]
async fn led_task(mut ws2812: Ws2812<'static, PIO0, 0, NUM_SMARTLEDS>) {
    // Tick every 100ms
    let mut ticker = Ticker::every(Duration::from_millis(100));
    let mut idx = 0;
    loop {
        // Wait for the next update time
        ticker.next().await;

        let mut colors = [colors::BLACK; NUM_SMARTLEDS];

        // A little iterator trickery to pick a moving set of four LEDs
        // to light up
        let (before, after) = colors.split_at_mut(idx);
        after
            .iter_mut()
            .chain(before.iter_mut())
            .take(4)
            .for_each(|l| {
                // The LEDs are very bright!
                *l = colors::BLUE / 16;
            });

        ws2812.write(&colors).await;
        idx += 1;
        if idx >= NUM_SMARTLEDS {
            idx = 0;
        }
    }
}
