#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use crate::{
    comms::init_sender,
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

use {defmt_rtt as _, panic_probe as _};

mod comms;
mod usb;

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
    let (d, ep_in, ep_out) = configure_usb(usb_r);

    let sender = init_sender(ep_in);
    spawner.must_spawn(usb_task(d));
    spawner.must_spawn(comms::rpc_dispatch(ep_out, sender));
}
