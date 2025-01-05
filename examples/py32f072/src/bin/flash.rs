#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::{info, unwrap};
use embassy_executor::Spawner;
use py32_hal::time::Hertz;
use py32_hal::mode::Blocking;
use py32_hal::flash::Flash;
use {defmt_rtt as _, panic_probe as _};

const TEST_DATA: [u8; 256] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
    30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
    57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83,
    84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108,
    109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130,
    131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152,
    153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174,
    175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196,
    197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218,
    219, 220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239, 240,
    241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255, 0
];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(Hertz::mhz(24));
    // cfg.rcc.pll = Some(Pll {
    //     src: PllSource::HSI,
    //     mul: PllMul::MUL3,
    // });
    // cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    info!("Hello Flash!");

    // Once can also call `into_regions()` to get access to NorFlash implementations
    // for each of the unique characteristics.
    let mut f = Flash::new_blocking(p.FLASH);

    test_flash(&mut f, 120 * 1024, 8 * 1024);

    test_flash(&mut f, 104 * 1024, 1 * 1024);

    test_flash(&mut f, 105 * 1024, 3 * 1024);
}

fn test_flash(f: &mut Flash<'_, Blocking>, offset: u32, size: u32) {
    info!("Testing offset: {=u32:#X}, size: {=u32:#X}", offset, size);

    info!("Reading...");
    let mut buf = [0u8; 32];
    unwrap!(f.blocking_read(offset, &mut buf));
    info!("Read: {=[u8]:x}", buf);

    info!("Erasing...");
    unwrap!(f.blocking_erase(offset, offset + size));

    info!("Reading...");
    let mut buf = [0u8; 32];
    unwrap!(f.blocking_read(offset, &mut buf));
    info!("Read after erase: {=[u8]:x}", buf);

    info!("Writing...");
    unwrap!(f.blocking_write(
        offset,
        &TEST_DATA
    ));

    info!("Reading...");
    let mut buf = [0u8; 32];
    unwrap!(f.blocking_read(offset, &mut buf));
    info!("Read: {=[u8]:x}", buf);
    assert_eq!(
        &buf[..],
        &TEST_DATA[..buf.len()],
    );
}
