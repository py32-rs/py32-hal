#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

// WARN
//
//
// py32f072 RingBuffered ADC Example
// Although DMA RingBuffer and ADC are functioning correctly,
// even with ADC clock divider set to 8 and CPU clock at 72MHz,
// the CPU read speed cannot keep up with the ADC data generation rate,
// resulting in Overrun errors.
// Therefore, this example code is not recommended for direct use.

use cortex_m::singleton;
use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Instant;
use py32_hal::adc::{Adc, RingBufferedAdc, SampleTime, Sequence};
use py32_hal::rcc::{HsiFs, Pll, PllMul, PllSource, Sysclk};
use py32_hal::Peripherals;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(HsiFs::HSI_24MHZ);
    cfg.rcc.pll = Some(Pll {
        src: PllSource::HSI,
        mul: PllMul::MUL3,
    });
    cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    spawner.must_spawn(adc_task(p));
}

#[embassy_executor::task]
async fn adc_task(mut p: Peripherals) {
    const ADC_BUF_SIZE: usize = 512;
    let adc_data: &mut [u16; ADC_BUF_SIZE] =
        singleton!(ADCDAT : [u16; ADC_BUF_SIZE] = [0u16; ADC_BUF_SIZE]).unwrap();

    let adc = Adc::new_with_prediv(p.ADC1, py32_hal::adc::Prescaler::Div8);
    let mut vrefint = adc.enable_vrefint();

    let mut adc: RingBufferedAdc<py32_hal::peripherals::ADC1> =
        adc.into_ring_buffered(p.DMA1_CH1, adc_data);

    adc.set_sample_sequence(Sequence::One, &mut p.PA0, SampleTime::CYCLES239_5);
    adc.set_sample_sequence(Sequence::Two, &mut p.PA2, SampleTime::CYCLES239_5);
    adc.set_sample_sequence(Sequence::Three, &mut p.PA1, SampleTime::CYCLES239_5);
    adc.set_sample_sequence(Sequence::Four, &mut p.PA3, SampleTime::CYCLES239_5);
    adc.set_sample_sequence(Sequence::Five, &mut vrefint, SampleTime::CYCLES239_5);

    // Note that overrun is a big consideration in this implementation. Whatever task is running the adc.read() calls absolutely must circle back around
    // to the adc.read() call before the DMA buffer is wrapped around > 1 time. At this point, the overrun is so significant that the context of
    // what channel is at what index is lost. The buffer must be cleared and reset. This *is* handled here, but allowing this to happen will cause
    // a reduction of performance as each time the buffer is reset, the adc & dma buffer must be restarted.

    // An interrupt executor with a higher priority than other tasks may be a good approach here, allowing this task to wake and read the buffer most
    // frequently.
    let mut tic = Instant::now();
    let mut buffer = [0u16; 256];
    let _ = adc.start();
    loop {
        match adc.read(&mut buffer).await {
            Ok(_data) => {
                let toc = Instant::now();
                info!(
                    "\n adc1: {} dt = {}, n = {}",
                    buffer[0..16],
                    (toc - tic).as_micros(),
                    _data
                );
                tic = toc;
            }
            Err(e) => {
                warn!("Error: {:?}", e);
                buffer = [0u16; 256];
                let _ = adc.start();
            }
        }
    }
}
