#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::uid;
use {defmt_rtt as _, panic_probe as _};

// The datasheet (seems to) specify a 128-bit UID, while the SDK uses 96-bit.
// We read the full 128 bits to be ensure uniqueness.

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello world!");
    let p = py32_hal::init(Default::default());
    info!("UID: {}", uid::uid());
}
