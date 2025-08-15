#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::usart::{Config, Uart};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello World!");

    // PA2 and PB6 are SWD pins, so reusing them may lock you out of programming.
    // Refer to the `unsafe-reuse-swd-pins` feature's comments in py32-hal/Cargo.toml.

    let p = py32_hal::init(Default::default());

    let config = Config::default();
    let mut usart = Uart::new_blocking(p.USART1, p.PA7, p.PA6, config).unwrap();

    unwrap!(usart.blocking_write(b"Hello Embassy World!"));
    info!("wrote Hello, starting echo");

    let mut buf = [0u8; 1];
    loop {
        unwrap!(usart.blocking_read(&mut buf));
        unwrap!(usart.blocking_write(&buf));
    }
}
