#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::crc::Crc;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());

    info!("Hello World!");

    let mut crc = Crc::new(p.CRC);

    // CRC-32 (poly 0x4C11DB7, init 0xFFFF_FFFF, no reflection).
    // Words are fed MSB-first, i.e. a word stream is equivalent to the
    // big-endian byte stream "12345678".
    crc.feed_words(&[0x31323334, 0x35363738]);
    let result = crc.read();
    info!("crc(\"12345678\") = {:#010x}", result);
    if result != 0x49e3_c2fb {
        defmt::panic!("crc mismatch");
    }

    // Feed one more word after a reset.
    crc.reset();
    crc.feed_word(0x41424344); // "ABCD", MSB-first like the vector above
    let result = crc.read();
    info!("crc(\"ABCD\") = {:#010x}", result);
    if result != 0xabcf_9a63 {
        defmt::panic!("crc mismatch");
    }

    info!("Test success");
}
