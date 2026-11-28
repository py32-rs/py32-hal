// Adapted from embassy-stm32.
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32

//! Direct Memory Access (DMA)
#![macro_use]

mod dma;

pub use dma::*;

mod util;
pub(crate) use util::*;

pub(crate) mod ringbuffer;
pub mod word;

use core::marker::PhantomData;

use embassy_hal_internal::{Peri, PeripheralType};

use crate::_generated::DmaChannel;
use crate::interrupt;

/// The direction of a DMA transfer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Dir {
    /// Transfer from memory to a peripheral.
    MemoryToPeripheral,
    /// Transfer from a peripheral to memory.
    PeripheralToMemory,
    /// Transfer from memory to another memory address.
    MemoryToMemory,
}

/// Which pointer in the transfer to increment.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Increment {
    /// DMA will not increment either of the addresses.
    None,
    /// DMA will increment the peripheral address.
    Peripheral,
    /// DMA will increment the memory address.
    Memory,
    /// DMA will increment both peripheral and memory addresses simultaneously.
    Both,
}

/// DMA request selected through the PY32 SYSCFG routing registers.
pub type Request = u8;

/// Ring buffer error.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum RingBufferError {
    /// Overrun error
    Overrun,
}

impl From<ringbuffer::Error> for RingBufferError {
    fn from(e: ringbuffer::Error) -> Self {
        if e == ringbuffer::Error::DmaUnsynced {
            error!("Ring buffer broken invariants detected!");
        }
        Self::Overrun
    }
}

/// DMA channel driver
pub struct Channel<'d> {
    pub(crate) channel: DmaChannel,
    phantom: PhantomData<&'d ()>,
}

impl<'d> Channel<'d> {
    /// Create a new DMA channel driver.
    pub fn new<T: ChannelInstance>(
        _ch: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, crate::dma::InterruptHandler<T>> + 'd,
    ) -> Self {
        Self {
            channel: T::CHANNEL,
            phantom: PhantomData,
        }
    }

    /// Reborrow the channel, allowing it to be used in multiple places.
    pub fn reborrow(&mut self) -> Channel<'_> {
        Channel {
            channel: self.channel,
            phantom: PhantomData,
        }
    }

    pub(crate) unsafe fn clone_unchecked(&self) -> Channel<'d> {
        Channel {
            channel: self.channel,
            phantom: PhantomData,
        }
    }
}

pub(crate) trait SealedChannelInstance {
    const CHANNEL: DmaChannel;
}

/// DMA channel.
#[allow(private_bounds)]
pub trait ChannelInstance: SealedChannelInstance + PeripheralType + 'static {
    /// The interrupt type for this DMA channel.
    type Interrupt: interrupt::typelevel::Interrupt;
}

/// DMA interrupt handler.
#[allow(private_bounds)]
pub struct InterruptHandler<T: ChannelInstance> {
    _marker: PhantomData<T>,
}

impl<T: ChannelInstance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        on_irq(T::CHANNEL)
    }
}

macro_rules! dma_channel_impl {
    ($channel_peri:ident, $irq:ty) => {
        impl crate::dma::SealedChannelInstance for crate::peripherals::$channel_peri {
            const CHANNEL: crate::_generated::DmaChannel =
                crate::_generated::DmaChannel::$channel_peri;
        }

        impl crate::dma::ChannelInstance for crate::peripherals::$channel_peri {
            type Interrupt = $irq;
        }
    };
}

const CHANNEL_COUNT: usize = crate::_generated::DMA_CHANNELS.len();
static STATE: [ChannelState; CHANNEL_COUNT] = [ChannelState::NEW; CHANNEL_COUNT];

pub(crate) fn info(channel: DmaChannel) -> &'static ChannelInfo {
    &crate::_generated::DMA_CHANNELS[channel as usize]
}

// Safety: called only once during HAL initialization.
pub(crate) unsafe fn init(
    cs: critical_section::CriticalSection,
    dma_priority: interrupt::Priority,
) {
    unsafe {
        dma::init(cs, dma_priority);
    }
}
