#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::gpio::{Level, Output, Speed};
use {defmt_rtt as _, panic_halt as _};

// main is itself an async function.
#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    // PA2 and PB6 are SWD pins, so reusing them may lock you out of programming.
    // Refer to the `unsafe-reuse-swd-pins` feature's comments in py32-hal/Cargo.toml.

    let mut led = Output::new(p.PA1, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        Timer::after_millis(300).await;

        info!("low");
        led.set_low();
        Timer::after_millis(300).await;
    }
}
