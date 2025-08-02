use embassy_rp::{
    bind_interrupts,
    dma::{self},
    peripherals::PIO0,
    pio::{Common, Instance, PioPin, StateMachine},
    pio_programs::ws2812::{PioWs2812, PioWs2812Program},
    Peri,
};
use embassy_time::Timer;
use smart_leds::RGB8;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(pub struct Irqs {
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
});

pub struct Ws2812<'d, P: Instance, const S: usize, const N: usize> {
    ws2812: PioWs2812<'d, P, S, N>,
}

impl<'d, P: Instance, const S: usize, const N: usize> Ws2812<'d, P, S, N> {
    pub fn new(
        pio: &mut Common<'d, P>,
        sm: StateMachine<'d, P, S>,
        dma: Peri<'d, impl dma::Channel>,
        pin: Peri<'d, impl PioPin>,
    ) -> Self {
        let program = PioWs2812Program::new(pio);
        let ws2812 = PioWs2812::new(pio, sm, dma, pin, &program);
        Self { ws2812 }
    }

    pub async fn write(&mut self, colors: &[RGB8; N]) {
        self.ws2812.write(colors).await;

        Timer::after_micros(55).await;
    }
}
