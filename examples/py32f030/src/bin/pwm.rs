#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use py32_hal::gpio::OutputType;
use py32_hal::time::khz;
use py32_hal::timer::simple_pwm::{PwmPin, SimplePwm};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = py32_hal::init(Default::default());
    info!("Hello World!");

    let ch4_pin = PwmPin::new_ch4(p.PA1, OutputType::PushPull);
    let mut pwm = SimplePwm::new(
        p.TIM1,
        None,
        None,
        None,
        Some(ch4_pin),
        khz(10),
        Default::default(),
    );
    let mut ch4 = pwm.ch4();
    ch4.enable();

    info!("PWM initialized");
    info!("PWM max duty {}", ch4.max_duty_cycle());

    loop {
        ch4.set_duty_cycle_fully_off();
        Timer::after_millis(300).await;
        ch4.set_duty_cycle_fraction(1, 4);
        Timer::after_millis(300).await;
        ch4.set_duty_cycle_fraction(1, 2);
        Timer::after_millis(300).await;
        ch4.set_duty_cycle(ch4.max_duty_cycle() - 1);
        Timer::after_millis(300).await;
    }
}
