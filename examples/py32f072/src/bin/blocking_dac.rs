#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::dac::{Dac, u12r};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    let mut dac = Dac::new_blocking(p.DAC, p.PA4, p.PA5);
    dac.set((u12r::new(0), u12r::new(0)));
    let (mut ch1, mut ch2) = dac.split();

    loop {
        // Drive both outputs from approximately 0 V to VDDA in ten equal steps.
        for step in 0..=10 {
            let value = (4095 * step / 10) as u16;
            ch1.set(u12r::new(value));
            ch2.set(u12r::new(value));
            info!("DAC outputs: PA4=PA5={}/4095", value);
            Timer::after_secs(1).await;
        }
    }
}
