#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::div::Div;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());

    info!("Hello World!");

    let mut div = Div::new(p.DIV);

    // Unsigned division (example from RM ch.20).
    let (quot, rem) = div.divide_unsigned(0x7250A3FB, 0x257D);
    info!("0x7250A3FB / 0x257D = {:#x} rem {:#x}", quot, rem);
    if quot != 0x30CA2 || rem != 0xEE1 {
        defmt::panic!("division mismatch");
    }

    // Signed division.
    let (quot, rem) = div.divide_signed(-1400, 100);
    info!("-1400 / 100 = {} rem {}", quot, rem);
    if quot != -14 || rem != 0 {
        defmt::panic!("division mismatch");
    }

    // Division by zero: the operation ends immediately with (0, 0)
    // and the ZERO flag is set.
    let (quot, rem) = div.divide_unsigned(42, 0);
    info!("42 / 0 = {} rem {} (zero={})", quot, rem, div.zero());
    if quot != 0 || rem != 0 || !div.zero() {
        defmt::panic!("division mismatch");
    }

    info!("Test success");
}
