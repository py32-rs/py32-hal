#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{Hsidiv, Mco, McoPrescaler, McoSource};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    // cfg.rcc.hsi = Some(HsiFs::HSI_8MHZ); // default
    cfg.rcc.hsidiv = Hsidiv::DIV4;
    // cfg.rcc.sys = Sysclk::HSI; // default
    let p = py32_hal::init(cfg);

    info!("Hello World!");

    let mut led = Output::new(p.PA6, Level::High, Speed::Low);

    // PA1 can act as MCO and is available on most packages, alternatively use
    // PA5, PA8 or PA9.
    let _mco = Mco::new(p.MCO, p.PA1, McoSource::SYSCLK, McoPrescaler::DIV1);

    loop {
        info!("high");
        led.set_high();
        // Note that the delay implementation assumes two cycles for a loop
        // consisting of a SUBS and BNE instruction, but the Cortex-M0+ uses
        // 3 cycles. The following value should give a flashing frequency of
        // about 1Hz.
        cortex_m::asm::delay(2_000_000 / 3);

        info!("low");
        led.set_low();
        cortex_m::asm::delay(2_000_000 / 3);
    }
}
