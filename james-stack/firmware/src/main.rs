#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use core::hash::{Hash, Hasher};

use crate::{
    comms::comms_task,
    usb::{configure_usb, usb_task, UsbResources},
};
use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{
    rcc::{
        AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPreDiv, PllSource, Pllp, Pllq,
        Sysclk,
    },
    time::Hertz,
    Config,
};
use james_icd::Sleep;
// use pd_core::HashWrap;
use postcard::experimental::schema::Schema;

use {defmt_rtt as _, panic_probe as _};

mod comms;
mod usb;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Hello World!");

    // use core::fmt::Write;
    // let mut buf = heapless::String::<1024>::new();
    // write!(&mut buf, "{:?}", Sleep::SCHEMA).ok();
    // info!("{=str}", &buf);

    // let mut hw = HashWrap::new();
    // "hello, world".hash(&mut hw);
    // let hash = hw.finish();
    // info!("{=u64}", hash);

    // let mut hw = HashWrap::new();
    // Sleep::SCHEMA.hash(&mut hw);
    // let hash = hw.finish();
    // info!("{=u64}", hash);

    // defmt::panic!("BYE");

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
            divp: Some(Pllp::DIV2), // 12mhz / 6 * 168 / 2 = 168Mhz.
            divq: Some(Pllq::DIV7), // 12mhz / 6 * 168 / 7 = 48Mhz.
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
        config
    };

    let p = embassy_stm32::init(config);

    let usb_r = UsbResources {
        periph: p.USB_OTG_FS,
        dp: p.PA12,
        dm: p.PA11,
    };
    let (d, c) = configure_usb(usb_r);

    spawner.must_spawn(usb_task(d));
    spawner.must_spawn(comms_task(c));
}
