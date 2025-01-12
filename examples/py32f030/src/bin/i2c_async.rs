#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::i2c::I2c;
use py32_hal::time::Hertz;
use py32_hal::{bind_interrupts, i2c, peripherals};
use {defmt_rtt as _, panic_probe as _};

const ADDRESS: u8 = 0x42;

bind_interrupts!(struct Irqs {
    I2C1 => i2c::GlobalInterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello world!");
    let p = py32_hal::init(Default::default());

    let mut i2c = I2c::new(
        p.I2C1,
        p.PA3,
        p.PA2,
        Irqs,
        p.DMA1_CH2,
        p.DMA1_CH1,
        Hertz(100_000),
        Default::default(),
    );

    loop {
        let write_data = [0xC2, 0x11];
        let mut read_data_buffer: [u8; 1] = [0];

        match i2c
            .write_read(ADDRESS, &write_data, &mut read_data_buffer)
            .await
        {
            Ok(()) => {
                info!("Data: {}", read_data_buffer);
            }
            Err(e) => error!("I2C Error during read: {:?}", e),
        }
    }
}
