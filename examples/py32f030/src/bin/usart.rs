#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use embassy_executor::Spawner;
use defmt::*;
use py32_hal::usart::{Config, Uart};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello World!");

    let p = py32_hal::init(Default::default());

    let config = Config::default();
    let mut usart = Uart::new_blocking(p.USART1, p.PA3, p.PA2, config).unwrap();

    unwrap!(usart.blocking_write(b"Hello Embassy World!"));
    info!("wrote Hello, starting echo");

    let mut buf = [0u8; 1];
    loop {
        unwrap!(usart.blocking_read(&mut buf));
        unwrap!(usart.blocking_write(&buf));
    }
}
