#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use py32_hal::bind_interrupts;
use py32_hal::gpio::{Input, Level, Output, Pull, Speed};
use py32_hal::rcc::{Pll, PllMul, PllSource, Sysclk, HsiFs};
use py32_hal::usb::{self, InterruptHandler};
use {defmt_rtt as _, panic_probe as _};

use usb_device::{class_prelude::*, prelude::*};
use usbd_human_interface_device::page::Keyboard;
use usbd_human_interface_device::prelude::*;

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
    let button = Input::new(p.PB0, Pull::Up);

    let usb_bus = usb::new_bus(p.USB, Irqs, p.PA12, p.PA11);

    let usb_bus_allocator = UsbBusAllocator::new(usb_bus);

    let mut keyboard = UsbHidClassBuilder::new()
        .add_device(usbd_human_interface_device::device::keyboard::BootKeyboardConfig::default())
        .build(&usb_bus_allocator);
    
    //https://pid.codes
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus_allocator, UsbVidPid(0x1209, 0x0001))
        .strings(&[StringDescriptors::default()
            .manufacturer("py32-rs team")
            .product("Boot Keyboard")
            .serial_number("TEST")])
        .unwrap()
        .build();

    let mut button_pressed = false;

    loop {
        if button.is_high() {
            if button_pressed {
                // Button was just released
                button_pressed = false;
                // Send release report with no keys pressed
                match keyboard.device().write_report([Keyboard::NoEventIndicated]) {
                    Err(UsbHidError::WouldBlock) => {}
                    Err(UsbHidError::Duplicate) => {}
                    Ok(_) => {}
                    Err(e) => {
                        core::panic!("Failed to write keyboard report: {:?}", e)
                    }
                };
            }
        } else {
            if !button_pressed {
                // Button was just pressed
                button_pressed = true;
                info!("Button pressed");
                // Send press report with 'A' key
                match keyboard.device().write_report([Keyboard::A]) {
                    Err(UsbHidError::WouldBlock) => {}
                    Err(UsbHidError::Duplicate) => {}
                    Ok(_) => {}
                    Err(e) => {
                        core::panic!("Failed to write keyboard report: {:?}", e)
                    }
                };
            }
        }

        //Tick once per ms
        match keyboard.tick() {
            Err(UsbHidError::WouldBlock) => {}
            Ok(_) => {}
            Err(e) => {
                core::panic!("Failed to process keyboard tick: {:?}", e)
            }
        };

        if usb_dev.poll(&mut [&mut keyboard]) {
            match keyboard.device().read_report() {
                Err(UsbError::WouldBlock) => {
                    //do nothing
                }
                Err(e) => {
                    core::panic!("Failed to read keyboard report: {:?}", e)
                }
                Ok(leds) => {
                    led.set_level(Level::from(leds.caps_lock));
                }
            }
        }
    }
}