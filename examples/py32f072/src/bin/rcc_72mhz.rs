#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{HsiFs, Pll, PllMul, PllSource, Sysclk};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(HsiFs::HSI_24MHZ);
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
        // Note that the delay implementation assumes two cycles for a loop
        // consisting of a SUBS and BNE instruction. The Cortex-M0+ normally
        // would use 3 cycles, but due to flash wait states necessary at high
        // SYSCLK speeds we are even slower.
        cortex_m::asm::delay(8_000_000);

        info!("low");
        led.set_low();
        cortex_m::asm::delay(8_000_000);
    }
}
