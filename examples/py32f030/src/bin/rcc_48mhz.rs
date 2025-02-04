#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{HsiFs, Pll, PllSource, Sysclk};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(HsiFs::HSI_24MHZ);
    cfg.rcc.pll = Some(Pll {
        src: PllSource::HSI,
    });
    cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    info!("Hello World!");

    let mut led = Output::new(p.PB1, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        // Note that the delay implementation assumes two cycles for a loop
        // consisting of a SUBS and BNE instruction. The Cortex-M0+ normally
        // would use 3 cycles, but due to flash wait states necessary at high
        // SYSCLK speeds we are even slower. The following value should give a
        // flashing frequency of about 1Hz.
        cortex_m::asm::delay(9_600_000);

        info!("low");
        led.set_low();
        cortex_m::asm::delay(9_600_000);
    }
}
