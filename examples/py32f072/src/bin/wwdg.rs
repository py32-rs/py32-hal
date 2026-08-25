#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::wdg::WindowWatchdog;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());

    info!("Hello World!");

    // Timeout of 8 ms, window closed for the first 2 ms.
    let mut wwdg = WindowWatchdog::new(p.WWDG, 8_000, 2_000);

    loop {
        embassy_time::Timer::after_millis(5).await;
        info!("petting watchdog");
        wwdg.pet();
    }
}
