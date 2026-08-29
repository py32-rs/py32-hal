//! DIV hardware divider

use crate::peripherals::DIV;
use crate::{Peri, rcc};

/// Hardware divider driver.
///
/// 32-bit signed/unsigned integer divider. A division takes 8 clock cycles
/// to complete. The peripheral has no interrupt; the driver polls the
/// `STAT.END` flag.
pub struct Div<'d> {
    _peri: Peri<'d, DIV>,
}

impl<'d> Div<'d> {
    /// Instantiates the DIV peripheral.
    pub fn new(peripheral: Peri<'d, DIV>) -> Self {
        // Note: enable and reset come from RccPeripheral.
        // enable DIV clock in RCC.
        rcc::enable_and_reset::<DIV>();
        Self { _peri: peripheral }
    }

    fn divide_blocking(&mut self, dividend: u32, divisor: u32, signed: bool) -> (u32, u32) {
        let div = crate::pac::DIV;
        div.sign().write(|w| w.set_sign(signed));
        div.dend().write_value(dividend);
        // Writing the divisor starts the division.
        div.sor().write_value(divisor);
        while !div.stat().read().end() {}
        (div.quot().read(), div.remd().read())
    }

    /// Unsigned division. Returns `(quotient, remainder)`.
    ///
    /// If `divisor` is 0 the operation ends immediately with `(0, 0)` and the
    /// division-by-zero flag is set; check it with [`Self::zero`].
    pub fn divide_unsigned(&mut self, dividend: u32, divisor: u32) -> (u32, u32) {
        self.divide_blocking(dividend, divisor, false)
    }

    /// Signed division. Returns `(quotient, remainder)`.
    ///
    /// If `divisor` is 0 the operation ends immediately with `(0, 0)` and the
    /// division-by-zero flag is set; check it with [`Self::zero`].
    pub fn divide_signed(&mut self, dividend: i32, divisor: i32) -> (i32, i32) {
        let (quot, rem) = self.divide_blocking(dividend as u32, divisor as u32, true);
        (quot as i32, rem as i32)
    }

    /// Returns `true` if the last completed division had a zero divisor.
    pub fn zero(&self) -> bool {
        crate::pac::DIV.stat().read().zero()
    }
}

impl<'d> Drop for Div<'d> {
    fn drop(&mut self) {
        rcc::disable::<DIV>();
    }
}
