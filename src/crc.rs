//! CRC (Cyclic Redundancy Check) calculation unit

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

use crate::peripherals::CRC;
use crate::{Peri, rcc};

/// CRC driver.
///
/// The peripheral computes a CRC-32 (polynomial 0x4C11DB7, init 0xFFFF_FFFF,
/// no input/output reversal) over 32-bit words fed MSB-first, taking 4 AHB
/// clock cycles per word.
pub struct Crc<'d> {
    _peri: Peri<'d, CRC>,
}

impl<'d> Crc<'d> {
    /// Instantiates the CRC peripheral and initializes it to default values.
    pub fn new(peripheral: Peri<'d, CRC>) -> Self {
        // Note: enable and reset come from RccPeripheral.
        // enable CRC clock in RCC.
        rcc::enable_and_reset::<CRC>();
        let mut instance = Self { _peri: peripheral };
        instance.reset();
        instance
    }

    /// Resets the CRC unit to default value (0xFFFF_FFFF)
    pub fn reset(&mut self) {
        crate::pac::CRC.cr().write(|w| w.set_reset(true));
    }

    /// Feeds a word into the CRC peripheral.
    pub fn feed_word(&mut self, word: u32) {
        crate::pac::CRC.dr().write_value(word);
    }

    /// Feeds a slice of words into the CRC peripheral.
    pub fn feed_words(&mut self, words: &[u32]) {
        for word in words {
            crate::pac::CRC.dr().write_value(*word);
        }
    }

    /// Read the CRC result value.
    pub fn read(&self) -> u32 {
        crate::pac::CRC.dr().read()
    }
}

impl<'d> Drop for Crc<'d> {
    fn drop(&mut self) {
        rcc::disable::<CRC>();
    }
}
