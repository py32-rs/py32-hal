#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{LsConfig, Sysclk};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.ls = LsConfig::default_lsi();
    cfg.rcc.sys = Sysclk::LSI;
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
