use core::mem::MaybeUninit;

use critical_section::CriticalSection;

use crate::pac::RCC;
// pub use crate::_generated::{mux, Clocks};
pub use crate::_generated::mux;
use crate::time::Hertz;

mod f030;
pub use f030::*;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub hclk1: crate::time::MaybeHertz,
    pub pclk1: crate::time::MaybeHertz,
    pub pclk1_tim: crate::time::MaybeHertz,
    // pub pclk2: crate::time::MaybeHertz,
    // pub pclk2_tim: crate::time::MaybeHertz,
    pub sys: crate::time::MaybeHertz,
    
    pub hsi: crate::time::MaybeHertz,
    pub lse: crate::time::MaybeHertz,
    // pub rtc: crate::time::MaybeHertz,
    // pub sys: Option<crate::time::Hertz>,
    // pub usb: Option<crate::time::Hertz>,
}


// #[cfg(feature = "low-power")]
// /// Must be written within a critical section
// ///
// /// May be read without a critical section
// pub(crate) static mut REFCOUNT_STOP1: u32 = 0;

// #[cfg(feature = "low-power")]
// /// Must be written within a critical section
// ///
// /// May be read without a critical section
// pub(crate) static mut REFCOUNT_STOP2: u32 = 0;

/// Frozen clock frequencies
///
/// The existence of this value indicates that the clock configuration can no longer be changed
static mut CLOCK_FREQS: MaybeUninit<Clocks> = MaybeUninit::uninit();

/// Sets the clock frequencies
///
/// Safety: Sets a mutable global.
pub(crate) unsafe fn set_freqs(freqs: Clocks) {
    debug!("rcc: {:?}", freqs);
    CLOCK_FREQS = MaybeUninit::new(freqs);
}

/// Safety: Reads a mutable global.
pub(crate) unsafe fn get_freqs() -> &'static Clocks {
    (*core::ptr::addr_of_mut!(CLOCK_FREQS)).assume_init_ref()
}

pub(crate) trait SealedRccPeripheral {
    fn frequency() -> Hertz;
    const RCC_INFO: RccInfo;
}

#[allow(private_bounds)]
pub trait RccPeripheral: SealedRccPeripheral + 'static {}

/// Runtime information necessary to reset, enable and disable a peripheral.
pub(crate) struct RccInfo {
    /// Offset in 32-bit words of the xxxRSTR register into the RCC register block, or 0xff if the
    /// peripheral has no reset bit (we don't use an `Option` to save one byte of storage).
    reset_offset_or_0xff: u8,
    /// Position of the xxxRST bit within the xxxRSTR register (0..=31).
    reset_bit: u8,
    /// Offset in 32-bit words of the xxxENR register into the RCC register block.
    enable_offset: u8,
    /// Position of the xxxEN bit within the xxxENR register (0..=31).
    enable_bit: u8,
    /// If this peripheral shares the same xxxRSTR bit and xxxEN bit with other peripherals, we
    /// maintain a refcount in `crate::_generated::REFCOUNTS` at this index. If the bit is not
    /// shared, this is 0xff (we don't use an `Option` to save one byte of storage).
    refcount_idx_or_0xff: u8,
    // /// Stop mode of the peripheral, used to maintain `REFCOUNT_STOP1` and `REFCOUNT_STOP2`.
    // #[cfg(feature = "low-power")]
    // stop_mode: StopMode,
}

// #[cfg(feature = "low-power")]
// #[allow(dead_code)]
// pub(crate) enum StopMode {
//     Standby,
//     Stop2,
//     Stop1,
// }

impl RccInfo {
    /// Safety:
    /// - `reset_offset_and_bit`, if set, must correspond to valid xxxRST bit
    /// - `enable_offset_and_bit` must correspond to valid xxxEN bit
    /// - `refcount_idx`, if set, must correspond to valid refcount in `_generated::REFCOUNTS`
    /// - `stop_mode` must be valid
    pub(crate) const unsafe fn new(
        reset_offset_and_bit: Option<(u8, u8)>,
        enable_offset_and_bit: (u8, u8),
        refcount_idx: Option<u8>,
        // #[cfg(feature = "low-power")] stop_mode: StopMode,
    ) -> Self {
        let (reset_offset_or_0xff, reset_bit) = match reset_offset_and_bit {
            Some((offset, bit)) => (offset, bit),
            None => (0xff, 0xff),
        };
        let (enable_offset, enable_bit) = enable_offset_and_bit;
        let refcount_idx_or_0xff = match refcount_idx {
            Some(idx) => idx,
            None => 0xff,
        };
        Self {
            reset_offset_or_0xff,
            reset_bit,
            enable_offset,
            enable_bit,
            refcount_idx_or_0xff,
            // #[cfg(feature = "low-power")]
            // stop_mode,
        }
    }

    // TODO: should this be `unsafe`?
    pub(crate) fn enable_and_reset_with_cs(&self, _cs: CriticalSection) {
        if self.refcount_idx_or_0xff != 0xff {
            let refcount_idx = self.refcount_idx_or_0xff as usize;

            // Use .get_mut instead of []-operator so that we control how bounds checks happen.
            // Otherwise, core::fmt will be pulled in here in order to format the integer in the
            // out-of-bounds error.
            if let Some(refcount) =
                unsafe { (*core::ptr::addr_of_mut!(crate::_generated::REFCOUNTS)).get_mut(refcount_idx) }
            {
                *refcount += 1;
                if *refcount > 1 {
                    return;
                }
            } else {
                panic!("refcount_idx out of bounds: {}", refcount_idx)
            }
        }

        // #[cfg(feature = "low-power")]
        // match self.stop_mode {
        //     StopMode::Standby => {}
        //     StopMode::Stop2 => unsafe {
        //         REFCOUNT_STOP2 += 1;
        //     },
        //     StopMode::Stop1 => unsafe {
        //         REFCOUNT_STOP1 += 1;
        //     },
        // }

        // set the xxxRST bit
        let reset_ptr = self.reset_ptr();
        if let Some(reset_ptr) = reset_ptr {
            unsafe {
                let val = reset_ptr.read_volatile();
                reset_ptr.write_volatile(val | 1u32 << self.reset_bit);
            }
        }

        // set the xxxEN bit
        let enable_ptr = self.enable_ptr();
        unsafe {
            let val = enable_ptr.read_volatile();
            enable_ptr.write_volatile(val | 1u32 << self.enable_bit);
        }

        // we must wait two peripheral clock cycles before the clock is active
        // this seems to work, but might be incorrect
        // see http://efton.sk/STM32/gotcha/g183.html

        // dummy read (like in the ST HALs)
        let _ = unsafe { enable_ptr.read_volatile() };

        // DSB for good measure
        cortex_m::asm::dsb();

        // clear the xxxRST bit
        if let Some(reset_ptr) = reset_ptr {
            unsafe {
                let val = reset_ptr.read_volatile();
                reset_ptr.write_volatile(val & !(1u32 << self.reset_bit));
            }
        }
    }

    // TODO: should this be `unsafe`?
    pub(crate) fn disable_with_cs(&self, _cs: CriticalSection) {
        if self.refcount_idx_or_0xff != 0xff {
            let refcount_idx = self.refcount_idx_or_0xff as usize;

            // Use .get_mut instead of []-operator so that we control how bounds checks happen.
            // Otherwise, core::fmt will be pulled in here in order to format the integer in the
            // out-of-bounds error.
            if let Some(refcount) =
                unsafe { (*core::ptr::addr_of_mut!(crate::_generated::REFCOUNTS)).get_mut(refcount_idx) }
            {
                *refcount -= 1;
                if *refcount > 0 {
                    return;
                }
            } else {
                panic!("refcount_idx out of bounds: {}", refcount_idx)
            }
        }

        // #[cfg(feature = "low-power")]
        // match self.stop_mode {
        //     StopMode::Standby => {}
        //     StopMode::Stop2 => unsafe {
        //         REFCOUNT_STOP2 -= 1;
        //     },
        //     StopMode::Stop1 => unsafe {
        //         REFCOUNT_STOP1 -= 1;
        //     },
        // }

        // clear the xxxEN bit
        let enable_ptr = self.enable_ptr();
        unsafe {
            let val = enable_ptr.read_volatile();
            enable_ptr.write_volatile(val & !(1u32 << self.enable_bit));
        }
    }

    // TODO: should this be `unsafe`?
    pub(crate) fn enable_and_reset(&self) {
        critical_section::with(|cs| self.enable_and_reset_with_cs(cs))
    }

    // TODO: should this be `unsafe`?
    pub(crate) fn disable(&self) {
        critical_section::with(|cs| self.disable_with_cs(cs))
    }

    fn reset_ptr(&self) -> Option<*mut u32> {
        if self.reset_offset_or_0xff != 0xff {
            Some(unsafe { (RCC.as_ptr() as *mut u32).add(self.reset_offset_or_0xff as _) })
        } else {
            None
        }
    }

    fn enable_ptr(&self) -> *mut u32 {
        unsafe { (RCC.as_ptr() as *mut u32).add(self.enable_offset as _) }
    }
}

#[allow(unused)]
mod util {
    use crate::time::Hertz;

    pub fn calc_pclk<D>(hclk: Hertz, ppre: D) -> (Hertz, Hertz)
    where
        Hertz: core::ops::Div<D, Output = Hertz>,
    {
        let pclk = hclk / ppre;
        let pclk_tim = if hclk == pclk { pclk } else { pclk * 2u32 };
        (pclk, pclk_tim)
    }

    pub fn all_equal<T: Eq>(mut iter: impl Iterator<Item = T>) -> bool {
        let Some(x) = iter.next() else { return true };
        if !iter.all(|y| y == x) {
            return false;
        }
        true
    }

    pub fn get_equal<T: Eq>(mut iter: impl Iterator<Item = T>) -> Result<Option<T>, ()> {
        let Some(x) = iter.next() else { return Ok(None) };
        if !iter.all(|y| y == x) {
            return Err(());
        }
        Ok(Some(x))
    }
}

/// Get the kernel clock frequency of the peripheral `T`.
///
/// # Panics
///
/// Panics if the clock is not active.
pub fn frequency<T: RccPeripheral>() -> Hertz {
    T::frequency()
}

/// Enables and resets peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
// TODO: should this be `unsafe`?
pub fn enable_and_reset_with_cs<T: RccPeripheral>(cs: CriticalSection) {
    T::RCC_INFO.enable_and_reset_with_cs(cs);
}

/// Disables peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
// TODO: should this be `unsafe`?
pub fn disable_with_cs<T: RccPeripheral>(cs: CriticalSection) {
    T::RCC_INFO.disable_with_cs(cs);
}

/// Enables and resets peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
// TODO: should this be `unsafe`?
pub fn enable_and_reset<T: RccPeripheral>() {
    T::RCC_INFO.enable_and_reset();
}

/// Disables peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
// TODO: should this be `unsafe`?
pub fn disable<T: RccPeripheral>() {
    T::RCC_INFO.disable();
}
