/// Universal Serial Bus (USB)
///
/// The USB peripheral IP in PY32 is a mini Mentor USB (musb),
/// featuring a fixed FIFO size and with some register functionalities masked.
///
/// See more: https://github.com/decaday/musb

use core::marker::PhantomData;

#[cfg(feature = "embassy-usb-driver-impl")]
use embassy_usb_driver as driver;
#[cfg(feature = "embassy-usb-driver-impl")]
use embassy_usb_driver::EndpointType;
#[cfg(feature = "embassy-usb-driver-impl")]
use musb::{MusbDriver, In, Out, Bus, ControlPipe, Endpoint};
#[cfg(feature = "usb-device-impl")]
pub use musb::UsbdBus;

use musb::UsbInstance;

use crate::interrupt::typelevel::Interrupt;
use crate::rcc::{self, RccPeripheral};
use crate::{interrupt, Peripheral};



/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        musb::on_interrupt::<UsbInstance>();
    }
}

fn init<T: Instance>() {
    let freq = T::frequency();
    if freq.0 != 48_000_000 {
        panic!("USB clock (PLL) must be 48MHz");
    }

    T::Interrupt::unpend();
    unsafe { T::Interrupt::enable() };
    rcc::enable_and_reset::<T>();

    #[cfg(feature = "time")]
    embassy_time::block_for(embassy_time::Duration::from_millis(100));
    #[cfg(not(feature = "time"))]
    cortex_m::asm::delay(unsafe { crate::rcc::get_freqs() }.sys.to_hertz().unwrap().0 / 10);
}

#[cfg(feature = "embassy-usb-driver-impl")]
/// USB driver.
pub struct Driver<'d, T: Instance> {
    phantom: PhantomData<&'d mut T>,
    inner: MusbDriver<'d, UsbInstance>,
}

#[cfg(feature = "embassy-usb-driver-impl")]
impl<'d, T: Instance> Driver<'d, T> {
    /// Create a new USB driver.
    pub fn new(
        _usb: impl Peripheral<P = T> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        _dp: impl Peripheral<P = impl DpPin<T>> + 'd,
        _dm: impl Peripheral<P = impl DmPin<T>> + 'd,
    ) -> Self {
        init::<T>();

        Self {
            inner: MusbDriver::new(),
            phantom: PhantomData,
        }
    }
}

#[cfg(feature = "embassy-usb-driver-impl")]
impl<'d, T: Instance> driver::Driver<'d> for Driver<'d, T> {
    type EndpointOut = Endpoint<'d, UsbInstance, Out>;
    type EndpointIn = Endpoint<'d, UsbInstance, In>;
    type ControlPipe = ControlPipe<'d, UsbInstance>;
    type Bus = Bus<'d, UsbInstance>;

    fn alloc_endpoint_in(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointIn, driver::EndpointAllocError> {
        self.inner
            .alloc_endpoint(ep_type, max_packet_size, interval_ms, None)
    }

    fn alloc_endpoint_out(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointOut, driver::EndpointAllocError> {
        self.inner
            .alloc_endpoint(ep_type, max_packet_size, interval_ms, None)
    }

    fn start(
        self,
        control_max_packet_size: u16,
    ) -> (Bus<'d, UsbInstance>, ControlPipe<'d, UsbInstance>) {
        self.inner.start(control_max_packet_size)
    }
}

#[cfg(feature = "usb-device-impl")]
pub fn new_bus<'d, T: Instance>(
    _usb: impl Peripheral<P = T> + 'd,
    _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    _dp: impl Peripheral<P = impl DpPin<T>> + 'd,
    _dm: impl Peripheral<P = impl DmPin<T>> + 'd,
) -> UsbdBus<UsbInstance> {
    init::<T>();

    UsbdBus::new()
}

trait SealedInstance {}

/// USB instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + RccPeripheral + 'static {
    /// Interrupt for this USB instance.
    type Interrupt: interrupt::typelevel::Interrupt;
}

// Internal PHY pins
pin_trait!(DpPin, Instance);
pin_trait!(DmPin, Instance);

foreach_interrupt!(
    ($inst:ident, usb, $block:ident, LP, $irq:ident) => {
        impl SealedInstance for crate::peripherals::$inst {}

        impl Instance for crate::peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$irq;
        }
    };
);
