#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::rcc::{Mco, McoPrescaler, McoSource};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());

    info!("Clock output (MCO)");

    let source = McoSource::SYSCLK;
    // let source = McoSource::HSI;
    // let source = McoSource::HSE;
    // let source = McoSource::PLL_CLK;
    // let source = McoSource::LSI;
    // let source = McoSource::LSE;

    let prescaler = McoPrescaler::DIV1;
    // let prescaler = McoPrescaler::DIV2;
    // let prescaler = McoPrescaler::DIV4;
    // let prescaler = McoPrescaler::DIV8;
    // let prescaler = McoPrescaler::DIV16;
    // let prescaler = McoPrescaler::DIV32;
    // let prescaler = McoPrescaler::DIV64;
    // let prescaler = McoPrescaler::DIV128;

    // PA1 can act as MCO and is available on most packages, alternatively use
    // PA5, PA8 or PA9.
    let _mco = Mco::new(p.MCO, p.PA1, source, prescaler);

    loop {}
}
