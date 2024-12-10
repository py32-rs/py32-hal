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

    let mut button = ExtiInput::new(p.PB5, p.EXTI5, Pull::Up);
    // let mut button = ExtiInput::new(p.PF4, p.EXTI4, Pull::None); // BOOT button
    // let mut button = ExtiInput::new(p.PA12, p.EXTI12, Pull::Up);

    info!("Press the USER button...");

    loop {
        button.wait_for_falling_edge().await;
        info!("Pressed!");
        button.wait_for_rising_edge().await;
        info!("Released!");
    }
}
