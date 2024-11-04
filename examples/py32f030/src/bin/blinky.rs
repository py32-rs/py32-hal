#![no_std]
#![no_main]

use defmt::*;
use py32_hal::gpio::{Level, Output, Speed};
use {defmt_rtt as _, panic_halt as _};
use cortex_m_rt::entry;

#[entry]
fn main() -> ! {
    let p = py32_hal::init();
    info!("Hello World!");
    //PA5 is the onboard LED on the Nucleo F091RC
    let mut led = Output::new(p.PB1, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        cortex_m::asm::delay(8_000_000);

        info!("low");
        led.set_low();

        cortex_m::asm::delay(8_000_000);
    }
}