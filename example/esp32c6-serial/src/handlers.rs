use defmt::info;
use postcard_rpc::header::VarHeader;
use smart_leds::{RGB8, SmartLedsWrite, brightness, gamma};
use workbook_icd::{BadPositionError, Rgb8, SingleLed};

use crate::app::{Context, LED_COUNT};

impl Context {
    fn update_led(&mut self, position: usize, rgb: Rgb8) -> Result<(), BadPositionError> {
        if position >= LED_COUNT {
            return Err(BadPositionError);
        }
        let data = RGB8 {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        };
        self.leds[position as usize] = brightness(gamma([data].into_iter()), 100).next().unwrap();
        Ok(())
    }

    fn write_leds(&mut self) -> Result<(), BadPositionError> {
        self.led.write(self.leds.iter().copied()).unwrap();
        Ok(())
    }
}

pub fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping");
    rqst
}

pub fn unique_id_handler(context: &mut Context, _header: VarHeader, _rqst: ()) -> u64 {
    info!("unique_id");
    context.unique_id
}

pub fn set_led_handler(
    context: &mut Context,
    _header: VarHeader,
    rqst: SingleLed,
) -> Result<(), BadPositionError> {
    info!("set_led");
    context.update_led(rqst.position as usize, rqst.rgb)?;
    context.write_leds()?;
    Ok(())
}

pub fn set_all_led_handler(context: &mut Context, _header: VarHeader, rqst: [Rgb8; 24]) {
    info!("set_all_led");
    for i in 0..LED_COUNT {
        context.update_led(i, rqst[i]).unwrap();
    }
    context.write_leds().unwrap();
}
