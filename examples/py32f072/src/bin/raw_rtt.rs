#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt::*;
use defmt_rtt as _;
use hal::pac;
use panic_halt as _;
use py32_hal as hal;

#[entry]
fn main() -> ! {
    info!("Hello World!");

    loop {
        info!("tick");

        cortex_m::asm::delay(8_000_000);
    }
}
