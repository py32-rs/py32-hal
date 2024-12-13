#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{Pll, PllMul, PllSource, Sysclk};
use py32_hal::time::Hertz;
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

    info!("Hello World!");

    let mut led = Output::new(p.PB2, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        cortex_m::asm::delay(8_000_000);

        info!("low");
        led.set_low();
        cortex_m::asm::delay(8_000_000);
    }
}
