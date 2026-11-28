#![no_std]
#![no_main]

use core::fmt::Write;

use defmt::*;
use embassy_executor::Spawner;
use heapless::String;
use py32_hal::usart::{Config, Uart};
use py32_hal::{bind_interrupts, peripherals, usart};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    DMA_CHANNEL1 => py32_hal::dma::InterruptHandler<peripherals::DMA1_CH1>;
    DMA_CHANNEL2_3 => py32_hal::dma::InterruptHandler<peripherals::DMA1_CH3>;
    USART2 => usart::InterruptHandler<peripherals::USART2>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    let config = Config::default();
    let mut usart =
        Uart::new(p.USART2, p.PA1, p.PA0, Irqs, p.DMA1_CH3, p.DMA1_CH1, config).unwrap();

    for n in 0u32.. {
        let mut s: String<128> = String::new();
        core::write!(&mut s, "Hello DMA World {}!\r\n", n).unwrap();

        unwrap!(usart.write(s.as_bytes()).await);
        info!("wrote DMA");
    }
}
