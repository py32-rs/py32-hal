#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::{info, panic};
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::bind_interrupts;
use py32_hal::mode::Async;
use py32_hal::peripherals;
use py32_hal::usart::{self, Config, RingBufferedUartRx, Uart, UartTx};
use {defmt_rtt as _, panic_probe as _};

const DMA_BUF_SIZE: usize = 256;

bind_interrupts!(struct Irqs {
    USART1 => usart::InterruptHandler<peripherals::USART1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    let config = Config::default();
    let usart = Uart::new(p.USART1, p.PA3, p.PA2, Irqs, p.DMA1_CH3, p.DMA1_CH1, config).unwrap();

    let (tx, rx) = usart.split();
    static mut DMA_BUF: [u8; DMA_BUF_SIZE] = [0; DMA_BUF_SIZE];
    let rx = rx.into_ring_buffered(unsafe { &mut *core::ptr::addr_of_mut!(DMA_BUF) });

    info!("Spawning tasks");
    spawner.spawn(transmit_task(tx)).unwrap();
    spawner.spawn(receive_task(rx)).unwrap();
}

#[embassy_executor::task]
async fn transmit_task(mut tx: UartTx<'static, Async>) {
    // workaround https://github.com/embassy-rs/embassy/issues/1426
    // I am not sure if py32 has same issue
    Timer::after_millis(100).await;

    info!("Starting sequential transmissions into void...");

    let mut i: u8 = 0;
    loop {
        let mut buf = [0; 256];
        let len = 16;
        for b in &mut buf[..len] {
            *b = i;
            i = i.wrapping_add(1);
        }

        tx.write(&buf[..len]).await.unwrap();
        Timer::after_millis(1000).await;
    }
}

#[embassy_executor::task]
async fn receive_task(mut rx: RingBufferedUartRx<'static>) {
    info!("Ready to receive...");
    let max_lens = [15, 32, 12, 55, 128];

    let mut i = 0;
    loop {
        let mut buf = [0; 256];

        let max_len = max_lens[i % max_lens.len()];
        let received = match rx.read(&mut buf[..max_len]).await {
            Ok(r) => r,
            Err(e) => {
                panic!("Read error: {:?}", e);
            }
        };
        info!("Received {} bytes", received);

        i += received;

        if i > 300 {
            info!("Test OK!");
            cortex_m::asm::bkpt();
        }
    }
}
