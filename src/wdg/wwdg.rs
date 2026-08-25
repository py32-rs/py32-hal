//! Window watchdog (WWDG)

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

use crate::pac::wwdg::vals::Wdgtb;
use crate::peripherals::WWDG;
use crate::{Peri, rcc};

/// Returns `ceil(duration_us * pclk1_hz / (prescaler_mul * 4096 * 1_000_000))`.
///
/// Uses `u64` arithmetic throughout to prevent overflow.
fn wwdg_ticks(duration_us: u32, pclk1_hz: u32, prescaler_mul: u32) -> u64 {
    let num = duration_us as u64 * pclk1_hz as u64;
    let den = prescaler_mul as u64 * 4096 * 1_000_000;
    (num + den - 1) / den
}

/// Window watchdog (WWDG) driver.
///
/// Once activated via [`WindowWatchdog::new`], the WWDG cannot be stopped
/// without a system reset.
///
/// The counter counts from `T` down to 0x3F (63), triggering a reset when it
/// reaches 0x3F. Petting the watchdog while the counter is still above the
/// window register `W` (the *closed window*) also causes an immediate reset.
///
/// ```text
/// T_initial ──count down──▶ W ──count down──▶ 0x40 ──▶ 0x3F (RESET)
/// |◄──── closed window ────►|◄──── open window ────►|
/// ```
pub struct WindowWatchdog<'d> {
    _peri: Peri<'d, WWDG>,
    /// Counter value written to CR on every [`pet`](WindowWatchdog::pet) call.
    counter: u8,
}

impl<'d> WindowWatchdog<'d> {
    /// Creates and immediately starts the window watchdog.
    ///
    /// - `timeout_us`: total watchdog period in microseconds (counter-to-reset time).
    /// - `window_us`: closed-window duration in microseconds. During this initial
    ///   portion of the period, petting the watchdog causes a reset. Pass `0` to
    ///   disable the window restriction (allow petting at any time within the period).
    ///   Must be strictly less than `timeout_us`.
    pub fn new(peripheral: Peri<'d, WWDG>, timeout_us: u32, window_us: u32) -> Self {
        assert!(window_us < timeout_us, "window_us must be less than timeout_us");

        rcc::enable_and_reset::<WWDG>();

        let pclk1 = rcc::frequency::<WWDG>().0;

        // Select the smallest prescaler such that ticks falls in [1, 64].
        const PRESCALER_MULS: &[u32] = &[1, 2, 4, 8];
        let (prescaler_mul, ticks) = unwrap!(PRESCALER_MULS.iter().find_map(|&mul| {
            let t = wwdg_ticks(timeout_us, pclk1, mul);
            if (1..=64).contains(&t) {
                Some((mul, t))
            } else {
                None
            }
        }));

        // T = 63 + ticks; T is in [0x40, 0x7F].
        let t_val = 63u8 + ticks as u8;

        // W = T − floor(window_us * pclk1 / (prescaler_mul * 4096 * 1_000_000)).
        // When window_us == 0 the closed window is empty and W == T.
        let den = prescaler_mul as u64 * 4096 * 1_000_000;
        let closed_ticks = (window_us as u64 * pclk1 as u64) / den;
        let w_val = t_val - closed_ticks as u8;

        // WDGTB bits are log2(prescaler_mul): DIV1=0, DIV2=1, DIV4=2, DIV8=3.
        let wdgtb = Wdgtb::from_bits(prescaler_mul.trailing_zeros() as u8);

        // Write CFR before CR: prescaler and window must be set before activation.
        crate::pac::WWDG.cfr().write(|cfr| {
            cfr.set_wdgtb(wdgtb);
            cfr.set_w(w_val);
        });

        // Activate watchdog (WDGA = 1 is hardware-irreversible).
        crate::pac::WWDG.cr().write(|cr| {
            cr.set_t(t_val);
            cr.set_wdga(true);
        });

        trace!(
            "WWDG configured: timeout={}us window={}us pclk1={} prescaler=x{} T={} W={}",
            timeout_us,
            window_us,
            pclk1,
            prescaler_mul,
            t_val,
            w_val,
        );

        Self {
            _peri: peripheral,
            counter: t_val,
        }
    }

    /// Pet (reload) the watchdog.
    ///
    /// Must be called while the counter has fallen into the open window
    /// (counter ≤ W). Calling too early (counter > W) causes an immediate reset.
    pub fn pet(&mut self) {
        crate::pac::WWDG.cr().write(|cr| {
            cr.set_t(self.counter);
            cr.set_wdga(true);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::wwdg_ticks;

    #[test]
    fn test_wwdg_ticks() {
        assert_eq!(wwdg_ticks(1000, 64_000_000, 1), 16);
        assert_eq!(wwdg_ticks(1024, 64_000_000, 1), 16);
        assert_eq!(wwdg_ticks(1025, 64_000_000, 1), 17);
        // ÷8 is the largest prescaler wwdg_v1 supports (2-bit WDGTB).
        assert_eq!(wwdg_ticks(30_000, 64_000_000, 8), 59);
        assert_eq!(wwdg_ticks(1, 64_000_000, 1), 1);
    }
}
