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
    info!("I2C slave example (PY32F002B)");
    let p = py32_hal::init(Default::default());

    // The PY32F002B has no DMA peripheral, so the transfers are driven by the
    // interrupts of the I2C peripheral.
    let mut config = Config::default();
    config.frequency = Hertz(100_000);

    let mut i2c = I2c::new_no_dma(p.I2C1, p.PB3, p.PB4, Irqs, config)
        .into_slave_multimaster(SlaveAddrConfig::basic(ADDRESS));

    let mut rx = [0u8; 16];
    let tx = *b"py32f002b";

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
