#![no_std]
#![no_main]

use core::fmt::Write;

use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::UsbDevice;
use heapless::String;
use py32_hal::adc::{Adc, SampleTime};
use py32_hal::peripherals::{ADC1, USB};
use py32_hal::rcc::{HsiFs, Pll, PllMul, PllSource, Sysclk};
use py32_hal::usb::{Driver, InterruptHandler as UsbInterruptHandler};
use py32_hal::{adc, bind_interrupts};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USB => UsbInterruptHandler<USB>;
    ADC_COMP => adc::InterruptHandler<ADC1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut cfg: py32_hal::Config = Default::default();

    // PY32 USB uses PLL as the clock source and can only run at 48MHz.
    cfg.rcc.hsi = Some(HsiFs::HSI_16MHZ);
    cfg.rcc.pll = Some(Pll {
        src: PllSource::HSI,
        mul: PllMul::MUL3,
    });
    cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    let mut adc = Adc::new_async(p.ADC1, Irqs);
    adc.set_sample_time(SampleTime::CYCLES71_5);
    let pin = p.PA7;
    let vrefint = adc.enable_vrefint();

    let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);

    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("py32-rs");
        config.product = Some("USB async ADC example");
        config.serial_number = Some("12345678");
        config.max_power = 100;
        config.max_packet_size_0 = 64;

        // Required for Windows compatibility.
        config.device_class = 0xEF;
        config.device_sub_class = 0x02;
        config.device_protocol = 0x01;
        config.composite_with_iads = true;
        config
    };

    let mut builder = {
        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        embassy_usb::Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.init([0; 256]),
            BOS_DESCRIPTOR.init([0; 256]),
            &mut [],
            CONTROL_BUF.init([0; 64]),
        )
    };

    let class = {
        static STATE: StaticCell<State> = StaticCell::new();
        CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64)
    };

    let usb = builder.build();
    let usb_fut = usb_task(usb);
    let adc_fut = adc_task(adc, pin, vrefint, class);

    join(usb_fut, adc_fut).await;
}

type MyUsbDriver = Driver<'static, USB>;
type MyUsbDevice = UsbDevice<'static, MyUsbDriver>;
type MyUsbClass = CdcAcmClass<'static, MyUsbDriver>;

async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}

async fn adc_task(
    mut adc: Adc<'static, ADC1>,
    mut pin: py32_hal::peripherals::PA7,
    mut vrefint: adc::VrefInt,
    mut class: MyUsbClass,
) -> ! {
    loop {
        class.wait_connection().await;
        info!("Connected");

        if sample_and_write(&mut adc, &mut pin, &mut vrefint, &mut class)
            .await
            .is_err()
        {
            info!("Disconnected");
        }
    }
}

struct Disconnected;

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => defmt::panic!("USB buffer overflow"),
            EndpointError::Disabled => Disconnected,
        }
    }
}

async fn sample_and_write(
    adc: &mut Adc<'static, ADC1>,
    pin: &mut py32_hal::peripherals::PA7,
    vrefint: &mut adc::VrefInt,
    class: &mut MyUsbClass,
) -> Result<(), Disconnected> {
    loop {
        let vrefint_sample = adc.read(vrefint).await;
        let sample = adc.read(pin).await;
        let millivolts = convert_to_millivolts(sample, vrefint_sample);

        let mut line: String<64> = String::new();
        unwrap!(writeln!(
            line,
            "adc_pa7={} vrefint={} adc_pa7_mv={}\r",
            sample, vrefint_sample, millivolts
        ));

        class.write_packet(line.as_bytes()).await?;
        Timer::after_millis(250).await;
    }
}

pub fn convert_to_millivolts(sample: u16, vrefint: u16) -> u16 {
    const VREFINT_MV: u32 = 1200; // mV

    (u32::from(sample) * VREFINT_MV / u32::from(vrefint)) as u16
}
