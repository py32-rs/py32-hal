#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::i2c::{Config, I2c, SlaveAddrConfig, SlaveCommandKind};
use py32_hal::time::Hertz;
use py32_hal::{bind_interrupts, i2c, peripherals};
use {defmt_rtt as _, panic_probe as _};

const ADDRESS: u8 = 0x42;

bind_interrupts!(struct Irqs {
    I2C1 => i2c::GlobalInterruptHandler<peripherals::I2C1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("I2C async slave example (DMA)");
    let p = py32_hal::init(Default::default());

    // TX uses DMA1_CH2 and RX uses DMA1_CH1, as in `i2c_async.rs`.
    // The async slave methods pick the DMA path when these channels are owned.
    let mut config = Config::default();
    config.frequency = Hertz(400_000);

    let mut i2c = I2c::new(p.I2C1, p.PA11, p.PA12, Irqs, p.DMA1_CH2, p.DMA1_CH1, config)
        .into_slave_multimaster(SlaveAddrConfig::basic(ADDRESS));

    let mut rx = [0u8; 32];
    let tx = *b"hello from py32";

    loop {
        match i2c.listen().await {
            Ok(cmd) => {
                info!(
                    "matched address 0x{:02x}, direction {:?}",
                    cmd.address, cmd.kind
                );
                match cmd.kind {
                    SlaveCommandKind::Write => match i2c.respond_to_write(&mut rx).await {
                        Ok(n) => info!("received {} bytes: {}", n, &rx[..n]),
                        Err(e) => error!("slave write failed: {:?}", e),
                    },
                    SlaveCommandKind::Read => match i2c.respond_to_read(&tx).await {
                        Ok(sent) => info!("sent {} bytes to the master", sent),
                        Err(e) => error!("slave read failed: {:?}", e),
                    },
                }
            }
            Err(e) => error!("listen failed: {:?}", e),
        }
    }
}
