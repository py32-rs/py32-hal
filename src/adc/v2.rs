use embassy_hal_internal::into_ref;

use super::blocking_delay_us;
use crate::adc::{Adc, AdcChannel, Instance, Resolution, SampleTime};
use crate::pac::adc::vals::Extsel;
use crate::peripherals::ADC;
// use crate::time::Hertz;
use crate::{rcc, Peripheral};
use crate::pac::RCC;

// mod ringbuffered_v2;
// pub use ringbuffered_v2::{RingBufferedAdc, Sequence};

/// Default VREF voltage used for sample conversion to millivolts.
pub const VREF_DEFAULT_MV: u32 = 3300;
/// VREF voltage used for factory calibration of VREFINTCAL register.
pub const VREF_CALIB_MV: u32 = 3300;

pub struct VrefInt;
impl AdcChannel<ADC> for VrefInt {}
impl super::SealedAdcChannel<ADC> for VrefInt {
    fn channel(&self) -> u8 {
        17
    }
}

impl VrefInt {
    /// Time needed for internal voltage reference to stabilize
    pub fn start_time_us() -> u32 {
        10
    }
}

pub struct Temperature;
impl AdcChannel<ADC> for Temperature {}
impl super::SealedAdcChannel<ADC> for Temperature {
    fn channel(&self) -> u8 {
        16
    }
}

impl Temperature {
    /// Time needed for temperature sensor readings to stabilize
    pub fn start_time_us() -> u32 {
        10
    }
}

// pub struct Vbat;
// impl AdcChannel<ADC> for Vbat {}
// impl super::SealedAdcChannel<ADC> for Vbat {
//     fn channel(&self) -> u8 {
//         18
//     }
// }

pub enum Prescaler {
    Div2,
    Div4,
    Div6,
    Div8,
}

impl Prescaler {
    //

    // fn from_pclk2(freq: Hertz) -> Self {
    //     // Datasheet for F2 specifies min frequency 0.6 MHz, and max 30 MHz (with VDDA 2.4-3.6V).
    //     #[cfg(stm32f2)]
    //     const MAX_FREQUENCY: Hertz = Hertz(30_000_000);
    //     // Datasheet for both F4 and F7 specifies min frequency 0.6 MHz, typ freq. 30 MHz and max 36 MHz.
    //     #[cfg(not(stm32f2))]
    //     const MAX_FREQUENCY: Hertz = Hertz(36_000_000);
    //     let raw_div = freq.0 / MAX_FREQUENCY.0;
    //     match raw_div {
    //         0..=1 => Self::Div2,
    //         2..=3 => Self::Div4,
    //         4..=5 => Self::Div6,
    //         6..=7 => Self::Div8,
    //         _ => panic!("Selected PCLK2 frequency is too high for ADC with largest possible prescaler."),
    //     }
    // }

    fn adcdiv(&self) -> crate::pac::rcc::vals::Adcdiv {
        match self {
            Prescaler::Div2 => crate::pac::rcc::vals::Adcdiv::DIV2,
            Prescaler::Div4 => crate::pac::rcc::vals::Adcdiv::DIV4,
            Prescaler::Div6 => crate::pac::rcc::vals::Adcdiv::DIV6,
            Prescaler::Div8 => crate::pac::rcc::vals::Adcdiv::DIV8,
        }
    }
}

impl<'d, T> Adc<'d, T>
where
    T: Instance,
{
    pub fn new(adc: impl Peripheral<P = T> + 'd, adc_div: Prescaler) -> Self {
        into_ref!(adc);
        rcc::enable_and_reset::<T>();

        // let presc = Prescaler::from_pclk2(T::frequency());
        RCC.cr().modify(|reg| {
            reg.set_adcdiv(adc_div.adcdiv());
        });
        T::regs().cr2().modify(|reg| {
            reg.set_extsel(Extsel::SWSTART);
        });

        T::regs().cr2().modify(|reg| {
            reg.set_adon(true);
        });

        blocking_delay_us(3);

        Self {
            adc,
            sample_time: SampleTime::from_bits(0),
        }
    }

    pub fn set_sample_time(&mut self, sample_time: SampleTime) {
        self.sample_time = sample_time;
    }

    pub fn set_resolution(&mut self, resolution: Resolution) {
        T::regs().cr1().modify(|reg| reg.set_res(resolution.into()));
    }

    /// Enables internal voltage reference and returns [VrefInt], which can be used in
    /// [Adc::read_internal()] to perform conversion.
    pub fn enable_vrefint(&self) -> VrefInt {
        T::regs().cr2().modify(|reg| {
            reg.set_tsvrefe(true);
        });

        VrefInt {}
    }

    /// Enables internal temperature sensor and returns [Temperature], which can be used in
    /// [Adc::read_internal()] to perform conversion.
    ///
    /// On STM32F42 and STM32F43 this can not be used together with [Vbat]. If both are enabled,
    /// temperature sensor will return vbat value.
    pub fn enable_temperature(&self) -> Temperature {
        T::regs().cr2().modify(|reg| {
            reg.set_tsvrefe(true);
        });

        Temperature {}
    }

    // /// Enables vbat input and returns [Vbat], which can be used in
    // /// [Adc::read_internal()] to perform conversion.
    // pub fn enable_vbat(&self) -> Vbat {
    //     T::common_regs().ccr().modify(|reg| {
    //         reg.set_vbate(true);
    //     });

    //     Vbat {}
    // }

    /// Perform a single conversion.
    fn convert(&mut self) -> u16 {
        // clear end of conversion flag
        T::regs().sr().modify(|reg| {
            reg.set_eoc(false);
        });

        // Start conversion
        T::regs().cr2().modify(|reg| {
            reg.set_swstart(true);
            reg.set_exttrig(true);
        });

        while T::regs().sr().read().strt() == false {
            // spin //wait for actual start
        }
        while T::regs().sr().read().eoc() == false {
            // spin //wait for finish
        }

        T::regs().dr().read().0 as u16
    }

    pub fn blocking_read(&mut self, channel: &mut impl AdcChannel<T>) -> u16 {
        channel.setup();

        // Configure ADC
        let channel = channel.channel();

        // Select channel
        T::regs().sqr3().write(|reg| reg.set_sq(0, channel));

        // Configure channel
        Self::set_channel_sample_time(channel, self.sample_time);

        self.convert()
    }

    fn set_channel_sample_time(ch: u8, sample_time: SampleTime) {
        let sample_time = sample_time.into();
        match ch {
            ..=9 => T::regs().smpr3().modify(|reg| reg.set_smp(ch as _, sample_time)),
            ..=19 => T::regs().smpr2().modify(|reg| reg.set_smp((ch - 10) as _, sample_time)),
            _ => T::regs().smpr3().modify(|reg| reg.set_smp((ch - 20) as _, sample_time)),
        }
    }
}

impl<'d, T: Instance> Drop for Adc<'d, T> {
    fn drop(&mut self) {
        T::regs().cr2().modify(|reg| {
            reg.set_adon(false);
        });

        rcc::disable::<T>();
    }
}
