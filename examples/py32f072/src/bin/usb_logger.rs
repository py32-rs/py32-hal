#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

// This example works well, but there is a dependency conflict.  
// Please remove the `embassy-usb` and `usbd-hid` dependencies from `Cargo.toml`, and then add:  
// ```toml
// embassy-usb-logger = "0.2.0"
// ```  
// This issue may be resolved in a future release of Embassy.

// Delete me
#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let _p = py32_hal::init(Default::default());
}


use {defmt_rtt as _, panic_probe as _};
// use embassy_executor::Spawner;
// use embassy_time::Timer;
// use py32_hal::bind_interrupts;
// use py32_hal::time::Hertz;
// use py32_hal::rcc::{Pll, PllSource, Sysclk, PllMul};
// use py32_hal::usb::{Driver, InterruptHandler};


// bind_interrupts!(struct Irqs {
//     USB => InterruptHandler<py32_hal::peripherals::USB>;
// });


// #[embassy_executor::task]
// async fn logger_task(driver: Driver<'static, py32_hal::peripherals::USB>) {
//     embassy_usb_logger::run!(512, log::LevelFilter::Info, driver);
// }

// #[embassy_executor::main]
// async fn main(spawner: Spawner) {
//     let mut cfg: py32_hal::Config = Default::default();

//     // PY32 USB uses PLL as the clock source and can only run at 48Mhz.
//     cfg.rcc.hsi = Some(Hertz::mhz(16));
//     cfg.rcc.pll = Some(Pll {
//         src: PllSource::HSI,
//         mul: PllMul::MUL3,
//     });
//     cfg.rcc.sys = Sysclk::PLL;
//     let p = py32_hal::init(cfg);

//     // Create the driver, from the HAL.

//     let driver = Driver::new(p.USB, Irqs, p.PA12, p.PA11);

//     spawner.spawn(logger_task(driver)).unwrap();

//     let mut counter = 0;
//     loop {
//         counter += 1;
//         log::info!("Tick {}", counter);
//         Timer::after_secs(1).await;
//     }
// }
