#![no_std]
#![no_main]

use defmt::*;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{PllSource, Pll, Sysclk};
use py32_hal::time::Hertz;
use {defmt_rtt as _, panic_halt as _};
use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(Hertz::mhz(24));
    cfg.rcc.pll = Some(Pll { src: PllSource::HSI });
    cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    info!("Hello World!");

    let mut led = Output::new(p.PB1, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        cortex_m::asm::delay(8_000_000);

        info!("low");
        led.set_low();

        cortex_m::asm::delay(8_000_000);
    }
}