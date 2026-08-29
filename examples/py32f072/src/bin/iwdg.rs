#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::wdg::IndependentWatchdog;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());

    info!("Hello World!");

    // 1 second timeout.
    let mut iwdg = IndependentWatchdog::new(p.IWDG, 1_000_000);

    info!("starting IWDG");
    iwdg.unleash();

    loop {
        embassy_time::Timer::after_millis(500).await;
        info!("petting watchdog");
        iwdg.pet();
    }
}
