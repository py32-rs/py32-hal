#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{Hse, HseMode, Sysclk};
use py32_hal::time::mhz;
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hse = Some(Hse {
        freq: mhz(24),
        mode: HseMode::Oscillator,
    });
    cfg.rcc.sys = Sysclk::HSE;
    let p = py32_hal::init(cfg);

    info!("Hello World!");

    let mut led = Output::new(p.PB2, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        Timer::after_millis(1000).await;

        info!("low");
        led.set_low();
        Timer::after_millis(1000).await;
    }
}
