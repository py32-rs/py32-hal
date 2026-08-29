//! Digital-to-analog converter (DAC).
//!
//! The DAC can be used as one independently owned channel with [`DacChannel`], or as both
//! channels together with [`Dac`]. This module provides blocking register access only.

#![macro_use]

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_hal_internal::{Peri, PeripheralType};

use crate::mode::{Blocking, Mode};
use crate::pac::dac::vals;
use crate::pac::dac::Dac as Regs;
use crate::peripherals;
use crate::rcc::{self, RccInfo, RccPeripheral, SealedRccPeripheral};

/// Software trigger source.
pub struct SOFTWARE;

impl<T: Instance> ChannelTrigger<T> for SOFTWARE {
    fn signal(&self) -> u8 {
        #[cfg(dac_v1)]
        const SOFTWARE_TRIGGER: u8 = 7;

        SOFTWARE_TRIGGER
    }
}

trigger_trait!(ChannelTrigger, Instance);

/// Channel 1 marker type.
pub enum Ch1 {}

/// Channel 2 marker type.
pub enum Ch2 {}

trait SealedChannel {
    const INDEX: usize;
}

/// DAC channel marker trait.
#[allow(private_bounds)]
pub trait Channel: SealedChannel {}

impl SealedChannel for Ch1 {
    const INDEX: usize = 0;
}

impl SealedChannel for Ch2 {
    const INDEX: usize = 1;
}

impl Channel for Ch1 {}
impl Channel for Ch2 {}

/// A pin that can carry a DAC channel's analog output.
pub trait DacPin<T: Instance, C: Channel>: crate::gpio::Pin {}

#[allow(unused_macros)]
macro_rules! impl_dac_pin {
    ($inst:ident, $pin:ident, 1u8) => {
        impl crate::dac::DacPin<crate::peripherals::$inst, crate::dac::Ch1>
            for crate::peripherals::$pin
        {
        }
    };
    ($inst:ident, $pin:ident, 2u8) => {
        impl crate::dac::DacPin<crate::peripherals::$inst, crate::dac::Ch2>
            for crate::peripherals::$pin
        {
        }
    };
}

/// A right-aligned 12-bit DAC sample.
///
/// Values passed to [`u12r::new`] are masked to 12 bits.
#[allow(non_camel_case_types)]
#[repr(transparent)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct u12r(pub u16);

impl u12r {
    /// Construct a right-aligned sample, masking it to 12 bits.
    pub const fn new(value: u16) -> Self {
        Self(value & 0x0fff)
    }
}

/// A left-aligned 12-bit DAC sample.
///
/// The contained value is the logical, unshifted 12-bit sample. The PAC field setter performs
/// the register alignment when the sample is written.
#[allow(non_camel_case_types)]
#[repr(transparent)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct u12l(pub u16);

impl u12l {
    /// Construct a left-aligned sample from a normal 12-bit value.
    pub const fn new(value: u16) -> Self {
        Self(value & 0x0fff)
    }
}

trait SealedWord: Sized {
    fn set_value(regs: Regs, index: usize, value: Self);
    fn set_values(regs: Regs, values: (Self, Self));
}

/// A sample representation supported by the DAC holding registers.
#[allow(private_bounds)]
pub trait Word: SealedWord {}

impl<T: SealedWord> Word for T {}

impl SealedWord for u8 {
    fn set_value(regs: Regs, index: usize, value: Self) {
        regs.dhr8r(index).write(|w| w.set_dhr(value));
    }

    fn set_values(regs: Regs, values: (Self, Self)) {
        regs.dhr8rd().write(|w| {
            w.set_dhr(0, values.0);
            w.set_dhr(1, values.1);
        });
    }
}

impl SealedWord for u12r {
    fn set_value(regs: Regs, index: usize, value: Self) {
        regs.dhr12r(index).write(|w| w.set_dhr(value.0));
    }

    fn set_values(regs: Regs, values: (Self, Self)) {
        regs.dhr12rd().write(|w| {
            w.set_dhr(0, values.0 .0);
            w.set_dhr(1, values.1 .0);
        });
    }
}

impl SealedWord for u12l {
    fn set_value(regs: Regs, index: usize, value: Self) {
        regs.dhr12l(index).write(|w| w.set_dhr(value.0));
    }

    fn set_values(regs: Regs, values: (Self, Self)) {
        regs.dhr12ld().write(|w| {
            w.set_dhr(0, values.0 .0);
            w.set_dhr(1, values.1 .0);
        });
    }
}

struct State {
    owners: AtomicU8,
}

impl State {
    const fn new() -> Self {
        Self {
            owners: AtomicU8::new(0),
        }
    }

    fn acquire(&self, count: u8) {
        critical_section::with(|_| {
            let owners = self.owners.load(Ordering::Relaxed);
            assert_eq!(owners, 0, "DAC peripheral is already owned");
            self.owners.store(count, Ordering::Relaxed);
        });
    }

    fn release(&self) -> bool {
        critical_section::with(|_| {
            let owners = self.owners.load(Ordering::Relaxed);
            debug_assert!(owners > 0);
            let remaining = owners - 1;
            self.owners.store(remaining, Ordering::Relaxed);
            remaining == 0
        })
    }
}

struct Info {
    regs: Regs,
    rcc: RccInfo,
}

trait SealedInstance {
    fn info() -> &'static Info;
    fn state() -> &'static State;
}

/// DAC peripheral instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + RccPeripheral + 'static {}

foreach_peripheral!(
    (dac, $inst:ident) => {
        impl crate::dac::SealedInstance for peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info {
                    regs: unsafe { Regs::from_ptr(crate::pac::$inst.as_ptr()) },
                    rcc: crate::peripherals::$inst::RCC_INFO,
                };
                &INFO
            }

            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl crate::dac::Instance for peripherals::$inst {}
    };
);

/// Driver for one DAC channel.
///
/// Use [`Dac`] when both output channels are required.
pub struct DacChannel<'d, M: Mode = Blocking> {
    info: &'static Info,
    state: &'static State,
    index: usize,
    _mode: PhantomData<&'d mut M>,
}

impl<'d> DacChannel<'d, Blocking> {
    /// Create a blocking DAC channel with triggering disabled and the output buffer enabled.
    pub fn new_blocking<T: Instance, C: Channel>(
        _peri: Peri<'d, T>,
        pin: Peri<'d, impl DacPin<T, C>>,
    ) -> Self {
        pin.set_as_analog();
        rcc::enable_and_reset::<T>();
        T::state().acquire(1);
        Self::new_inner::<T, C>(None)
    }

    /// Create a blocking DAC channel driven by the selected trigger source.
    pub fn new_blocking_triggered<T: Instance, C: Channel>(
        _peri: Peri<'d, T>,
        trigger: impl ChannelTrigger<T>,
        pin: Peri<'d, impl DacPin<T, C>>,
    ) -> Self {
        pin.set_as_analog();
        rcc::enable_and_reset::<T>();
        T::state().acquire(1);
        Self::new_inner::<T, C>(Some(trigger.signal()))
    }
}

impl<'d, M: Mode> DacChannel<'d, M> {
    fn new_inner<T: Instance, C: Channel>(trigger: Option<u8>) -> Self {
        let info = T::info();
        let index = C::INDEX;

        info.regs.cr().modify(|w| {
            w.set_en(index, false);
            w.set_boff(index, false);
            if let Some(trigger) = trigger {
                w.set_tsel(index, vals::Tsel::from_bits(trigger));
                w.set_ten(index, true);
            } else {
                w.set_ten(index, false);
            }
            w.set_wave(index, vals::Wave::DISABLED);
            w.set_dmaen(index, false);
            w.set_dmaudrie(index, false);
        });

        let mut channel = Self {
            info,
            state: T::state(),
            index,
            _mode: PhantomData,
        };
        channel.enable();
        channel
    }

    /// Enable or disable this channel.
    pub fn set_enable(&mut self, enabled: bool) {
        critical_section::with(|_| {
            self.info
                .regs
                .cr()
                .modify(|w| w.set_en(self.index, enabled));
        });
    }

    /// Enable this channel.
    pub fn enable(&mut self) {
        self.set_enable(true);
    }

    /// Disable this channel.
    pub fn disable(&mut self) {
        self.set_enable(false);
    }

    /// Issue a software trigger.
    pub fn trigger(&mut self) {
        self.info
            .regs
            .swtrigr()
            .write(|w| w.set_swtrig(self.index, true));
    }

    /// Write a new sample into this channel's holding register.
    ///
    /// With triggering disabled the sample transfers to the output register after one APB clock
    /// cycle. With triggering enabled, the next selected trigger transfers the sample.
    pub fn set<W: Word>(&mut self, value: W) {
        W::set_value(self.info.regs, self.index, value);
    }

    /// Read the current 12-bit output register value.
    pub fn read(&self) -> u16 {
        self.info.regs.dor(self.index).read().dor()
    }
}

impl<'d, M: Mode> Drop for DacChannel<'d, M> {
    fn drop(&mut self) {
        critical_section::with(|_| {
            self.info.regs.cr().modify(|w| {
                w.set_dmaen(self.index, false);
                w.set_en(self.index, false);
            });
        });
        if self.state.release() {
            self.info.rcc.disable();
        }
    }
}

/// Driver for both channels of a dual-channel DAC.
pub struct Dac<'d, M: Mode = Blocking> {
    info: &'static Info,
    ch1: DacChannel<'d, M>,
    ch2: DacChannel<'d, M>,
}

impl<'d> Dac<'d, Blocking> {
    /// Create a blocking dual-channel DAC with triggering disabled and output buffers enabled.
    pub fn new_blocking<T: Instance>(
        _peri: Peri<'d, T>,
        pin_ch1: Peri<'d, impl DacPin<T, Ch1>>,
        pin_ch2: Peri<'d, impl DacPin<T, Ch2>>,
    ) -> Self {
        pin_ch1.set_as_analog();
        pin_ch2.set_as_analog();
        rcc::enable_and_reset::<T>();
        T::state().acquire(2);

        Self {
            info: T::info(),
            ch1: DacChannel::new_inner::<T, Ch1>(None),
            ch2: DacChannel::new_inner::<T, Ch2>(None),
        }
    }

    /// Create a blocking dual-channel DAC driven by the selected trigger sources.
    pub fn new_blocking_triggered<T: Instance>(
        _peri: Peri<'d, T>,
        trigger_ch1: impl ChannelTrigger<T>,
        trigger_ch2: impl ChannelTrigger<T>,
        pin_ch1: Peri<'d, impl DacPin<T, Ch1>>,
        pin_ch2: Peri<'d, impl DacPin<T, Ch2>>,
    ) -> Self {
        pin_ch1.set_as_analog();
        pin_ch2.set_as_analog();
        rcc::enable_and_reset::<T>();
        T::state().acquire(2);

        Self {
            info: T::info(),
            ch1: DacChannel::new_inner::<T, Ch1>(Some(trigger_ch1.signal())),
            ch2: DacChannel::new_inner::<T, Ch2>(Some(trigger_ch2.signal())),
        }
    }
}

impl<'d, M: Mode> Dac<'d, M> {
    /// Split this DAC into independently movable channel handles.
    pub fn split(self) -> (DacChannel<'d, M>, DacChannel<'d, M>) {
        (self.ch1, self.ch2)
    }

    /// Borrow channel 1.
    pub fn ch1(&mut self) -> &mut DacChannel<'d, M> {
        &mut self.ch1
    }

    /// Borrow channel 2.
    pub fn ch2(&mut self) -> &mut DacChannel<'d, M> {
        &mut self.ch2
    }

    /// Write both holding registers with one atomic peripheral register write.
    pub fn set<W: Word>(&mut self, values: (W, W)) {
        W::set_values(self.info.regs, values);
    }
}
