#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = py32_hal::pac, peripherals = false, dispatchers = [TIM14])]
mod app {
    use defmt::info;
    use embassy_time::Timer;
    use py32_hal::gpio::{Level, Output, Speed};

    #[shared]
    struct Shared {}

    #[local]
    struct Local {}

    #[init]
    fn init(_: init::Context) -> (Shared, Local) {
        let p = py32_hal::init(Default::default());
        info!("Hello World!");

        let led = Output::new(p.PB2, Level::High, Speed::Low);
        blink::spawn(led).map_err(|_| ()).unwrap();

        (Shared {}, Local {})
    }

    #[task(priority = 1)]
    async fn blink(_cx: blink::Context, mut led: Output<'static>) {
        loop {
            info!("high");
            led.set_high();
            Timer::after_millis(1000).await;

            info!("low");
            led.set_low();
            Timer::after_millis(1000).await;
        }
    }
}
