#![cfg_attr(not(test), no_std)]
#![allow(async_fn_in_trait)]

// This must go FIRST so that all the other modules see its macros.
mod fmt;
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

mod macros;
use embassy_hal_internal::interrupt::Priority;
pub use py32_metapac as pac;

/// Operating modes for peripherals.
pub mod mode {
    trait SealedMode {}

    /// Operating mode for a peripheral.
    #[allow(private_bounds)]
    pub trait Mode: SealedMode {}

    macro_rules! impl_mode {
        ($name:ident) => {
            impl SealedMode for $name {}
            impl Mode for $name {}
        };
    }

    /// Blocking mode.
    pub struct Blocking;
    /// Async mode.
    pub struct Async;

    impl_mode!(Blocking);
    impl_mode!(Async);
}

pub mod adc;
pub mod dma;
pub mod flash;
pub mod gpio;
pub mod i2c;
pub mod rcc;
pub mod timer;
pub mod usart;

#[cfg(any(feature = "embassy-usb-driver-impl", feature = "usb-device-impl"))]
pub mod usb;

#[cfg(feature = "exti")]
pub mod exti;

pub mod embassy;
pub mod time;
#[cfg(feature = "time-driver-systick")]
pub use embassy::systick_time_driver;
#[cfg(all(feature = "_time-driver", not(feature = "time-driver-systick")))]
pub use embassy::time_driver;

#[cfg(feature = "time-driver-systick")]
use cortex_m::peripheral::SYST;

/// `py32-hal` global configuration.
#[non_exhaustive]
#[derive(Clone, Copy)]
pub struct Config {
    /// RCC config.
    pub rcc: rcc::Config,
    // /// Enable debug during sleep and stop.
    // ///
    // /// May increase power consumption. Defaults to true.
    // #[cfg(dbgmcu)]
    // pub enable_debug_during_sleep: bool,

    // /// BDMA interrupt priority.
    // ///
    // /// Defaults to P0 (highest).
    // #[cfg(bdma)]
    // pub bdma_interrupt_priority: Priority,
    /// DMA interrupt priority.
    ///
    /// Defaults to P0 (highest).
    #[cfg(dma)]
    pub dma_interrupt_priority: Priority,
    // /// GPDMA interrupt priority.
    // ///
    // /// Defaults to P0 (highest).
    // #[cfg(gpdma)]
    // pub gpdma_interrupt_priority: Priority,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rcc: Default::default(),
            // #[cfg(dbgmcu)]
            // enable_debug_during_sleep: true,
            // #[cfg(any(stm32l4, stm32l5, stm32u5))]
            // enable_independent_io_supply: true,
            // #[cfg(bdma)]
            // bdma_interrupt_priority: Priority::P0,
            #[cfg(dma)]
            dma_interrupt_priority: Priority::P0,
            // #[cfg(gpdma)]
            // gpdma_interrupt_priority: Priority::P0,
        }
    }
}

/// Initialize the `embassy-stm32` HAL with the provided configuration.
///
/// This returns the peripheral singletons that can be used for creating drivers.
///
/// This should only be called once at startup, otherwise it panics.
pub fn init(config: Config, #[cfg(feature = "time-driver-systick")] systick: SYST) -> Peripherals {
    critical_section::with(|cs| {
        let p = Peripherals::take_with_cs(cs);
        unsafe {
            rcc::init(config.rcc);
            crate::_generated::init_syscfg();

            gpio::init(cs);

            // must be after rcc init
            #[cfg(all(feature = "_time-driver", not(feature = "time-driver-systick")))]
            time_driver::init(cs);

            #[cfg(feature = "time-driver-systick")]
            systick_time_driver::init(cs, systick);

            #[cfg(feature = "exti")]
            exti::init(cs);

            dma::init(cs, config.dma_interrupt_priority);

            rcc::enable_and_reset_with_cs::<peripherals::FLASH>(cs);
        };
        p
    })
}

// This must go last, so that it sees all the impl_foo! macros defined earlier.
pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}

pub use crate::_generated::interrupt;

pub use _generated::{peripherals, Peripherals};
pub use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};

// developer note: this macro can't be in `embassy-hal-internal` due to the use of `$crate`.
#[macro_export]
macro_rules! bind_interrupts {
    ($vis:vis struct $name:ident {
        $(
            $(#[cfg($cond_irq:meta)])?
            $irq:ident => $(
                $(#[cfg($cond_handler:meta)])?
                $handler:ty
            ),*;
        )*
    }) => {
        #[derive(Copy, Clone)]
        $vis struct $name;

        $(
            #[allow(non_snake_case)]
            #[no_mangle]
            $(#[cfg($cond_irq)])?
            unsafe extern "C" fn $irq() {
                $(
                    $(#[cfg($cond_handler)])?
                    <$handler as $crate::interrupt::typelevel::Handler<$crate::interrupt::typelevel::$irq>>::on_interrupt();

                )*
            }
            $(#[cfg($cond_irq)])?
            $crate::bind_interrupts!(@inner
                $(
                    $(#[cfg($cond_handler)])?
                    unsafe impl $crate::interrupt::typelevel::Binding<$crate::interrupt::typelevel::$irq, $handler> for $name {}
                )*
            );
        )*
    };
    (@inner $($t:tt)*) => {
        $($t)*
    }
}
