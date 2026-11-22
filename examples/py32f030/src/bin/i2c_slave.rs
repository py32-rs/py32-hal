#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::i2c::{Config, I2c, SlaveAddrConfig, SlaveCommandKind};
use py32_hal::time::Hertz;
use {defmt_rtt as _, panic_probe as _};

const ADDRESS: u8 = 0x42;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("I2C slave example");
    let p = py32_hal::init(Default::default());

    let mut config = Config::default();
    config.frequency = Hertz(400_000);

    let mut i2c = I2c::new_blocking(p.I2C1, p.PA11, p.PA12, config)
        .into_slave_multimaster(SlaveAddrConfig::basic(ADDRESS));

    let mut rx = [0u8; 32];
    let tx = *b"hello from py32";

    loop {
        match i2c.blocking_listen() {
            Ok(cmd) => {
                info!(
                    "matched address 0x{:02x}, direction {:?}",
                    cmd.address, cmd.kind
                );
                match cmd.kind {
                    SlaveCommandKind::Write => match i2c.blocking_respond_to_write(&mut rx) {
                        Ok(n) => info!("received {} bytes: {}", n, &rx[..n]),
                        Err(e) => error!("slave write failed: {:?}", e),
                    },
                    SlaveCommandKind::Read => match i2c.blocking_respond_to_read(&tx) {
                        Ok(sent) => info!("sent {} bytes to the master", sent),
                        Err(e) => error!("slave read failed: {:?}", e),
                    },
                }
            }
            Err(e) => error!("listen failed: {:?}", e),
        }
    }
}
