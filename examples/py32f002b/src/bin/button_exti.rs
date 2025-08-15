#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::exti::ExtiInput;
use py32_hal::gpio::Pull;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    // PA2 and PB6 are SWD pins, so reusing them may lock you out of programming.
    // Refer to the `unsafe-reuse-swd-pins` feature's comments in py32-hal/Cargo.toml.

    let mut button = ExtiInput::new(p.PA0, p.EXTI0, Pull::Up);

    info!("Press the USER button...");

    loop {
        button.wait_for_falling_edge().await;
        info!("Pressed!");
        button.wait_for_rising_edge().await;
        info!("Released!");
    }
}
