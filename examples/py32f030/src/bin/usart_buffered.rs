#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::usart::{BufferedUart, Config};
use py32_hal::{bind_interrupts, peripherals, usart};
use py32_hal::time::Hertz;
use embedded_io_async::Read;
use embedded_io_async::Write;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USART1 => usart::BufferedInterruptHandler<peripherals::USART1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(Hertz::mhz(24));
    let p = py32_hal::init(cfg);
    info!("Hello World!");

    let config = Config::default();
    let mut tx_buf = [0u8; 256];
    let mut rx_buf = [0u8; 256];
    let mut usart = BufferedUart::new(p.USART1, Irqs, p.PA3, p.PA2, &mut tx_buf, &mut rx_buf, config).unwrap();

    usart.write_all(b"Hello Embassy World!\r\n").await.unwrap();
    info!("wrote Hello, starting echo");

    let mut buf = [0; 5];
    loop {
        // When using defmt, be cautious with the info! and other logging macros! 
        // If you're using a single channel (as is usually the case), defmt requires global_logger to acquire interrupts to be disabled. 
        // For example, defmt-rtt uses critical_section, which temporarily disables global interrupts. 
        //This can lead to USART Overrun error(SR.ORE), causing some data to be lost.
        usart.read_exact(&mut buf[..]).await.unwrap();
        // info!("Received:{} {}", buf, buf.len());
        usart.write_all(&buf[..]).await.unwrap();

        // use embedded_io_async::BufRead;
        // let buf = usart.fill_buf().await.unwrap();
        // let n = buf.len();
        // usart.consume(n);
    }
}
