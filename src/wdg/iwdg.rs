//! Independent watchdog (IWDG)

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

use crate::pac::iwdg::vals::{Key, Pr};
use crate::peripherals::IWDG;
use crate::rcc::LSI_FREQ;
use crate::Peri;

/// Maximum timeout that can be achieved with the prescaler set to 256 and the reload value set to 0xFFF.
pub const MAX_TIMEOUT_MICROS: u32 = 32_000_000;

const MAX_RL: u16 = 0xFFF;

/// Independent watchdog (IWDG) driver.
///
/// The watchdog is clocked by the internal 32.768 kHz LSI oscillator.
/// Once started, it can no longer be stopped.
pub struct IndependentWatchdog<'d> {
    _peri: Peri<'d, IWDG>,
}

impl<'d> IndependentWatchdog<'d> {
    /// Create a new IWDG instance with a given timeout value in microseconds.
    ///
    /// [Self] has to be started with [Self::unleash()].
    /// Once the timer expires, the MCU will be reset. To prevent this, the timer must be
    /// reloaded by repeatedly calling [Self::pet()] within the timeout interval.
    pub fn new(peripheral: Peri<'d, IWDG>, timeout_us: u32) -> Self {
        let psc_power = unwrap!((2..=8).find(|psc_power| {
            let psc = 2u16.pow(*psc_power);
            timeout_us <= get_timeout_us(psc, MAX_RL)
        }));

        // Feed the watchdog in case it's already running
        crate::pac::IWDG.kr().write(|w| w.set_key(Key::RESET));

        let psc = 2u16.pow(psc_power);
        let pr = psc_power - 2;

        // Enable register access
        crate::pac::IWDG.kr().write(|w| w.set_key(Key::ENABLE));

        // Configure prescaler and reload value
        crate::pac::IWDG.pr().write(|w| w.set_pr(Pr::from_bits(pr as u8)));
        crate::pac::IWDG.rlr().write(|w| w.set_rl(reload_value(psc, timeout_us)));

        Self { _peri: peripheral }
    }

    /// Unleash the watchdog.
    ///
    /// This will start the watchdog, and once started it cannot be stopped.
    pub fn unleash(&mut self) {
        crate::pac::IWDG.kr().write(|w| w.set_key(Key::START));
    }

    /// Pet the watchdog.
    pub fn pet(&mut self) {
        crate::pac::IWDG.kr().write(|w| w.set_key(Key::RESET));
    }
}

const fn get_timeout_us(prescaler: u16, reload_value: u16) -> u32 {
    1_000_000 * (reload_value + 1) as u32 / (LSI_FREQ.0 / prescaler as u32)
}

const fn reload_value(prescaler: u16, timeout_us: u32) -> u16 {
    (timeout_us / prescaler as u32 * LSI_FREQ.0 / 1_000_000) as u16 - 1
}

#[cfg(test)]
mod tests {
    use super::{MAX_RL, get_timeout_us, reload_value};

    #[test]
    fn can_compute_timeout_us() {
        assert_eq!(get_timeout_us(4, 0), 122);
        assert_eq!(get_timeout_us(4, MAX_RL), 500_000);
        assert_eq!(get_timeout_us(256, 0), 7812);
        assert_eq!(get_timeout_us(256, MAX_RL), 32_000_000);
        assert_eq!(get_timeout_us(64, 3999), 7_812_500);
    }

    #[test]
    fn can_compute_reload_value() {
        assert_eq!(reload_value(4, 500_000), 0xFFF);
        assert_eq!(reload_value(256, 32_000_000), 0xFFF);
        assert_eq!(reload_value(64, 8_000_000), 0xFFF);
    }
}
