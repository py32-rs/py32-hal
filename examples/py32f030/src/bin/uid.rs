#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::uid;
use py32_hal::time::Hertz;
use {defmt_rtt as _, panic_probe as _};

const ADDRESS: u8 = 0x42;
const WRITE_DATA: [u8; 2] = [0xC2, 0x10];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello world!");
    let p = py32_hal::init(Default::default());
    info!("UID: {}", uid::uid());
}
