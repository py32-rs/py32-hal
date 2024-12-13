#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::rcc::{Pll, PllMul, PllSource, Sysclk};
use py32_hal::time::Hertz;
use py32_hal::usart::{Config, Uart};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(Hertz::mhz(24));
    cfg.rcc.pll = Some(Pll {
        src: PllSource::HSI,
        mul: PllMul::MUL3,
    });
    cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    let config = Config::default();
    // let mut usart = Uart::new_blocking(p.USART2, p.PA3, p.PA2, config).unwrap();
    let mut usart = Uart::new_blocking(p.USART1, p.PA10, p.PA9, config).unwrap();

    unwrap!(usart.blocking_write(b"Hello Embassy World!"));
    info!("wrote Hello, starting echo");

    let mut buf = [0u8; 1];
    loop {
        unwrap!(usart.blocking_read(&mut buf));
        unwrap!(usart.blocking_write(&buf));
    }
}
