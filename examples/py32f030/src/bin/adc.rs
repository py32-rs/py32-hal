#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::adc::{Adc, SampleTime};
use py32_hal::peripherals::ADC;
use py32_hal::{adc, bind_interrupts};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ADC_COMP => adc::InterruptHandler<ADC>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    let mut adc = Adc::new(p.ADC, Irqs);
    adc.set_sample_time(SampleTime::CYCLES71_5);
    let mut pin = p.PA1;

    let mut vrefint = adc.enable_vref();
    let vrefint_sample = adc.read(&mut vrefint).await;
    let convert_to_millivolts = |sample| {
        const VREFINT_MV: u32 = 1200; // mV

        (u32::from(sample) * VREFINT_MV / u32::from(vrefint_sample)) as u16
    };

    loop {
        let v = adc.read(&mut pin).await;
        info!("vrefint_sample: {}", vrefint_sample);
        info!("--> {} - {} mV", v, convert_to_millivolts(v));
        Timer::after_millis(100).await;
    }
}
