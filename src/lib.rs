#![cfg_attr(not(test), no_std)]
#![allow(async_fn_in_trait)]

// This must go FIRST so that all the other modules see its macros.
mod fmt;
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

mod macros;

pub use py32_metapac as pac;

// This must go last, so that it sees all the impl_foo! macros defined earlier.
pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}

// pub use crate::_generated::interrupt;

pub use _generated::{peripherals, Peripherals};
pub use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};

pub mod time;
pub mod rcc;
pub mod gpio;

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

    // /// DMA interrupt priority.
    // ///
    // /// Defaults to P0 (highest).
    // #[cfg(dma)]
    // pub dma_interrupt_priority: Priority,

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
            // #[cfg(dma)]
            // dma_interrupt_priority: Priority::P0,
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
pub fn init(config: Config) -> Peripherals {
    critical_section::with(|cs| {
        let p = Peripherals::take_with_cs(cs);
        unsafe {
            rcc::init(config.rcc);
            gpio::init(cs);
        };
        p
    })
}