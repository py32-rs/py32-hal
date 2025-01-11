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

    let mut led = Output::new(p.PB2, Level::High, Speed::Low);

    info!("size_of Output = {}", core::mem::size_of::<Output>());
    info!(
        "size_of O Output = {}",
        core::mem::size_of::<Option<Output>>()
    );

    loop {
        info!("high");
        led.set_high();
        Timer::after_millis(1000).await;

        info!("low");
        led.set_low();
        Timer::after_millis(1000).await;
    }
}
