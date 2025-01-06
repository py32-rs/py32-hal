#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use py32_hal::bind_interrupts;
use py32_hal::gpio::{Level, Output, Speed};
use py32_hal::rcc::{Pll, PllMul, PllSource, Sysclk, HsiFs};
use py32_hal::usb::{self, InterruptHandler};
use {defmt_rtt as _, panic_probe as _};

use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

bind_interrupts!(struct Irqs {
    USB => InterruptHandler<py32_hal::peripherals::USB>;
});

#[cortex_m_rt::entry]
fn main() -> ! {
    let mut cfg: py32_hal::Config = Default::default();

    // PY32 USB uses PLL as the clock source and can only run at 48Mhz.
    cfg.rcc.hsi = Some(HsiFs::HSI_16MHZ);
    cfg.rcc.pll = Some(Pll {
        src: PllSource::HSI,
        mul: PllMul::MUL3,
    });
    cfg.rcc.sys = Sysclk::PLL;
    let p = py32_hal::init(cfg);

    let mut led = Output::new(p.PB2, Level::High, Speed::Low);

    let usb_bus = usb::new_bus(p.USB, Irqs, p.PA12, p.PA11);

    let usb_bus_allocator = UsbBusAllocator::new(usb_bus);

    let mut serial = SerialPort::new(&usb_bus_allocator);

    let string_descriptors = StringDescriptors::new(LangID::EN_US)
        .manufacturer("py32-rs team")
        .product("Serial")
        .serial_number("TEST");

    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus_allocator, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[string_descriptors])
        .unwrap()
        .max_packet_size_0(64)
        .unwrap()
        .device_class(USB_CLASS_CDC)
        .build();

    loop {
        if !usb_dev.poll(&mut [&mut serial]) {
            continue;
        }

        let mut buf = [0u8; 64];

        match serial.read(&mut buf) {
            Ok(count) if count > 0 => {
                led.set_high(); // Turn on

                info!("data: {:x}", &buf[0..count]);

                // Echo back in upper case
                for c in buf[0..count].iter_mut() {
                    if 0x61 <= *c && *c <= 0x7a {
                        *c &= !0x20;
                    }
                }

                let mut write_offset = 0;
                while write_offset < count {
                    match serial.write(&buf[write_offset..count]) {
                        Ok(len) if len > 0 => {
                            write_offset += len;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        led.set_low(); // Turn off
    }
}
