#![no_std]

use {
    defmt_rtt as _,
    embassy_rp::{
        adc::{self, Adc, Config as AdcConfig},
        bind_interrupts,
        flash::{Blocking, Flash},
        gpio::{Input, Level, Output, Pull},
        peripherals::{
            ADC, DMA_CH1, DMA_CH2, FLASH, PIN_0, PIN_1, PIN_18, PIN_19, PIN_2, PIN_20, PIN_21,
            PIN_26, PIN_3, PIN_4, PIN_5, PIN_6, PIN_7, SPI0, USB,
        },
        spi::{self, Async, Spi},
        usb,
    },
    embassy_time::Delay,
    embedded_hal_bus::spi::ExclusiveDevice,
    lis3dh_async::{Lis3dh, Lis3dhSPI},
    panic_probe as _,
};
pub mod ws2812;
use embassy_time as _;

bind_interrupts!(pub struct Irqs {
    ADC_IRQ_FIFO => adc::InterruptHandler;
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});

pub const NUM_SMARTLEDS: usize = 24;

/// Helper to get unique ID from flash
pub fn get_unique_id(flash: &mut FLASH) -> Option<u64> {
    let mut flash: Flash<'_, FLASH, Blocking, { 16 * 1024 * 1024 }> = Flash::new_blocking(flash);

    // TODO: For different flash chips, we want to handle things
    // differently based on their jedec? That being said: I control
    // the hardware for this project, and our flash supports unique ID,
    // so oh well.
    //
    // let jedec = flash.blocking_jedec_id().unwrap();

    let mut id = [0u8; core::mem::size_of::<u64>()];
    flash.blocking_unique_id(&mut id).unwrap();
    Some(u64::from_be_bytes(id))
}

pub struct Buttons {
    pub buttons: [Input<'static>; 8],
}

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
impl Buttons {
    pub const COUNT: usize = 8;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        b01: PIN_0,
        b02: PIN_1,
        b03: PIN_2,
        b04: PIN_3,
        b05: PIN_18,
        b06: PIN_19,
        b07: PIN_20,
        b08: PIN_21,
    ) -> Self {
        Self {
            buttons: [
                Input::new(b01, Pull::Up),
                Input::new(b02, Pull::Up),
                Input::new(b03, Pull::Up),
                Input::new(b04, Pull::Up),
                Input::new(b05, Pull::Up),
                Input::new(b06, Pull::Up),
                Input::new(b07, Pull::Up),
                Input::new(b08, Pull::Up),
            ],
        }
    }

    // Read all buttons, and report whether they are PRESSED, e.g. pulled low.
    pub fn read_all(&self) -> [bool; Self::COUNT] {
        let mut all = [false; Self::COUNT];
        all.iter_mut().zip(self.buttons.iter()).for_each(|(a, b)| {
            *a = b.is_low();
        });
        all
    }
}

// | GPIO Name | Usage                     | Notes                             |
// | :---      | :---                      | :---                              |
// | GPIO26    | ADC0                      | Potentiometer                     |
pub struct Potentiometer {
    pub adc: Adc<'static, adc::Async>,
    pub p26: adc::Channel<'static>,
}

impl Potentiometer {
    pub fn new(adc: ADC, pin: PIN_26) -> Self {
        let adc = Adc::new(adc, Irqs, AdcConfig::default());
        let p26 = adc::Channel::new_pin(pin, Pull::None);
        Self { adc, p26 }
    }

    /// Reads the ADC, returning a value between 0 and 4095.
    ///
    /// 0 is all the way to the right, and 4095 is all the way to the left
    pub async fn read(&mut self) -> u16 {
        let Ok(now) = self.adc.read(&mut self.p26).await else {
            defmt::panic!("Failed to read ADC!");
        };
        now
    }
}

// | GPIO Name | Usage                     | Notes                             |
// | :---      | :---                      | :---                              |
// | GPIO04    | SPI MISO/CIPO             | LIS3DH                            |
// | GPIO05    | SPI CSn                   | LIS3DH                            |
// | GPIO06    | SPI CLK                   | LIS3DH                            |
// | GPIO07    | SPI MOSI/COPI             | LIS3DH                            |
type AccSpi = Spi<'static, SPI0, Async>;
type ExclusiveSpi = ExclusiveDevice<AccSpi, Output<'static>, Delay>;
type Accel = Lis3dh<Lis3dhSPI<ExclusiveSpi>>;
pub struct Accelerometer {
    pub dev: Accel,
}

#[derive(Debug, PartialEq, defmt::Format)]
pub struct AccelReading {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl Accelerometer {
    pub async fn new(
        periph: SPI0,
        clk: PIN_6,
        copi: PIN_7,
        cipo: PIN_4,
        csn: PIN_5,
        tx_dma: DMA_CH1,
        rx_dma: DMA_CH2,
    ) -> Self {
        let mut cfg = spi::Config::default();
        cfg.frequency = 1_000_000;
        let spi = Spi::new(periph, clk, copi, cipo, tx_dma, rx_dma, cfg);
        let dev = ExclusiveDevice::new(spi, Output::new(csn, Level::High), Delay);
        let Ok(mut dev) = Lis3dh::new_spi(dev).await else {
            defmt::panic!("Failed to initialize SPI!");
        };
        if dev.set_range(lis3dh_async::Range::G8).await.is_err() {
            defmt::panic!("Error setting range!");
        };
        Self { dev }
    }

    pub async fn read(&mut self) -> AccelReading {
        let Ok(raw_acc) = self.dev.accel_raw().await else {
            defmt::panic!("Failed to get acceleration!");
        };
        AccelReading {
            x: raw_acc.x,
            y: raw_acc.y,
            z: raw_acc.z,
        }
    }
}
