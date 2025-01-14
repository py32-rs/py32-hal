use core::marker::PhantomData;

use embassy_hal_internal::into_ref;

use crate::gpio::{AfType, OutputType, Speed};
pub use crate::pac::rcc::vals::Mcopre as McoPrescaler;
pub use crate::pac::rcc::vals::Mcosel as McoSource;
use crate::pac::RCC;
use crate::{peripherals, Peripheral};

pub(crate) trait SealedMcoInstance {}

#[allow(private_bounds)]
pub trait McoInstance: SealedMcoInstance + 'static {
    type Source;

    #[doc(hidden)]
    unsafe fn _apply_clock_settings(source: Self::Source, prescaler: super::McoPrescaler);
}

pin_trait!(McoPin, McoInstance);

macro_rules! impl_peri {
    ($peri:ident, $source:ident, $set_source:ident, $set_prescaler:ident) => {
        impl SealedMcoInstance for peripherals::$peri {}
        impl McoInstance for peripherals::$peri {
            type Source = $source;

            unsafe fn _apply_clock_settings(source: Self::Source, _prescaler: McoPrescaler) {
                RCC.cfgr().modify(|w| {
                    w.$set_source(source);
                    w.$set_prescaler(_prescaler);
                });
            }
        }
    };
}

#[cfg(mco)]
impl_peri!(MCO, McoSource, set_mcosel, set_mcopre);

pub struct Mco<'d, T: McoInstance> {
    phantom: PhantomData<&'d mut T>,
}

impl<'d, T: McoInstance> Mco<'d, T> {
    /// Create a new MCO instance.
    pub fn new(
        _peri: impl Peripheral<P = T> + 'd,
        pin: impl Peripheral<P = impl McoPin<T>> + 'd,
        source: T::Source,
        prescaler: McoPrescaler,
    ) -> Self {
        into_ref!(pin);

        critical_section::with(|_| unsafe {
            T::_apply_clock_settings(source, prescaler);
            pin.set_as_af(
                pin.af_num(),
                AfType::output(OutputType::PushPull, Speed::VeryHigh),
            );
        });

        Self {
            phantom: PhantomData,
        }
    }
}
