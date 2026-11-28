#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use panic_probe as _;
use py32_hal::bind_interrupts;
use py32_hal::dma::{self, Channel, TransferOptions};
use py32_hal::peripherals::{self};

bind_interrupts! (struct Irqs{
    DMA_CHANNEL1 => dma::InterruptHandler<peripherals::DMA1_CH1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    // Generate a 32-element array with a distinct pattern for each index
    // This ensures we aren't accidentally transferring the same word 32 times
    // or skipping/offsetting any bytes.
    let source: [u32; 32] =
        core::array::from_fn(|i| (i as u32).wrapping_mul(0x01010101) ^ 0xDEADBEEF);

    let mut dest: [u32; 32] = [0; 32];

    let mut dma_ch = Channel::new(p.DMA1_CH1, Irqs);
    let tfer_opts = TransferOptions::default();

    println!("DMA starting...");
    println!(
        "Source [0]: {:#010x}, Source [31]: {:#010x}",
        source[0], source[31]
    );
    println!(
        "Dest   [0]: {:#010x}, Dest   [31]: {:#010x}",
        dest[0], dest[31]
    );

    // Memory-to-memory transfers do not use peripheral requests. Defaulting to 0.
    let tfer =
        unsafe { dma_ch.transfer::<u32, u32>(0, source.as_slice(), dest.as_mut_ptr(), tfer_opts) };

    tfer.await;

    println!("DMA finished.");
    println!(
        "Dest   [0]: {:#010x}, Dest   [31]: {:#010x}",
        dest[0], dest[31]
    );

    // Verify every single element
    let mut mismatch = false;
    for (i, (s, d)) in source.iter().zip(dest.iter()).enumerate() {
        if s != d {
            println!("Mismatch at index {}: src={:#010x}, dst={:#010x}", i, s, d);
            mismatch = true;
        }
    }

    if !mismatch {
        println!("Success: All 32 elements (128 bytes) transferred perfectly!");
    }

    loop {}
}
