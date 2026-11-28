#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::i2c::{Config, I2c};
use py32_hal::time::Hertz;
use py32_hal::{bind_interrupts, i2c, peripherals};
use {defmt_rtt as _, panic_probe as _};

const ADDRESS: u8 = 0x42;

bind_interrupts!(struct Irqs {
    DMA1_CHANNEL1 => py32_hal::dma::InterruptHandler<peripherals::DMA1_CH1>;
    DMA1_CHANNEL4_5_6_7 => py32_hal::dma::InterruptHandler<peripherals::DMA1_CH6>;
    I2C1 => i2c::GlobalInterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello world!");
    let p = py32_hal::init(Default::default());

    let mut config = Config::default();
    config.frequency = Hertz(100_000);

    let mut i2c = I2c::new(p.I2C1, p.PA9, p.PA10, Irqs, p.DMA1_CH6, p.DMA1_CH1, config);

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
