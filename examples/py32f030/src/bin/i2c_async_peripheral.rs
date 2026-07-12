#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::i2c::I2c;
use py32_hal::i2c::{Command, SlaveAddrConfig};
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
    ).into_slave_multimaster(SlaveAddrConfig { gencall: true, own_addr: 0x01 });;

    loop {
        let mut read_data_buffer: [u8; 16] = [0; 16];
        match i2c.listen(&mut read_data_buffer).await {
            Ok(Command::GeneralCall(n)) | Ok(Command::Write(n)) => {
                info!("Recieved {} bytes {} in write!", n, read_data_buffer);
            },
            Ok(Command::Read) => {
                let _ = i2c.respond_to_read(&[0x1], true).await;
                info!("Responded to read!");
            },
            Err(e) => {
                error!("I2C read error {}", e);
            }
        }
    }
}
