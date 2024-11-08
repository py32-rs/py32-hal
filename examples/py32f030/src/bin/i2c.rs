#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::i2c::{Error, I2c};
use py32_hal::time::Hertz;
use {defmt_rtt as _, panic_probe as _};

const ADDRESS: u8 = 0x5F;
const WHOAMI: u8 = 0x0F;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello world!");
    let p = py32_hal::init(Default::default());

    let mut i2c = I2c::new_blocking(p.I2C1, p.PF1, p.PF0, Hertz(100_000), Default::default());

    let mut data = [0u8; 1];

    match i2c.blocking_write_read(ADDRESS, &[WHOAMI], &mut data) {
        Ok(()) => info!("Whoami: {}", data[0]),
        Err(Error::Timeout) => error!("Operation timed out"),
        Err(e) => error!("I2c Error: {:?}", e),
    }
}
