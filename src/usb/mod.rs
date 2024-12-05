use core::future::poll_fn;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;

use embassy_hal_internal::into_ref;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver as driver;
use embassy_usb_driver::{
    Direction, EndpointAddress, EndpointAllocError, EndpointError, EndpointInfo, EndpointType, Event, Unsupported,
};

use crate::pac::usb::regs;
use crate::pac::usb::vals::Mode;
use crate::rcc::{self, RccPeripheral};
use crate::{interrupt, Peripheral};

mod endpoint;
pub use endpoint::Endpoint;
use endpoint::{EndpointData, EndPointConfig};

#[path ="driver.rs"]
mod usb_driver;
pub use usb_driver::Driver;

mod bus;
pub use bus::Bus;

mod control_pipe;
pub use control_pipe::ControlPipe;

#[cfg(py32f072)]
const EP_COUNT: usize = 6;
#[cfg(py32f403)]
const EP_COUNT: usize = 8;

const NEW_AW: AtomicWaker = AtomicWaker::new();
static BUS_WAKER: AtomicWaker = NEW_AW;
static EP0_SETUP: AtomicBool = AtomicBool::new(false);

// const NEW_CTR_TRIGGERED: AtomicBool = AtomicBool::new(false);
// static CTR_TRIGGERED: [AtomicBool; EP_COUNT] = [NEW_CTR_TRIGGERED; EP_COUNT];

static EP_IN_WAKERS: [AtomicWaker; EP_COUNT] = [NEW_AW; EP_COUNT];
static EP_OUT_WAKERS: [AtomicWaker; EP_COUNT] = [NEW_AW; EP_COUNT];
static IRQ_RESET: AtomicBool = AtomicBool::new(false);
static IRQ_SUSPEND: AtomicBool = AtomicBool::new(false);
static IRQ_RESUME: AtomicBool = AtomicBool::new(false);


fn calc_max_fifo_size(len: u16) -> u8 {
    let btyes = ((len + 7) / 8) as u8;
    if btyes > 8 {
        panic!("Invalid length: {}", len);
    }
    btyes
}

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let int_usb = T::regs().int_usb().read();
        if int_usb.reset() {
            IRQ_RESET.store(true, Ordering::SeqCst);
            BUS_WAKER.wake();
        }
        if int_usb.suspend() {
            IRQ_SUSPEND.store(true, Ordering::SeqCst);
            BUS_WAKER.wake();
        }
        if int_usb.resume() {
            IRQ_RESUME.store(true, Ordering::SeqCst);
            BUS_WAKER.wake();
        }

        let int_in = T::regs().int_in1().read();
        let int_out = T::regs().int_out1().read();
        if int_in.ep0() {
            // TODO
            // EP0_SETUP.store(true, Ordering::SeqCst);
            EP_IN_WAKERS[0].wake();
            EP_OUT_WAKERS[0].wake();
        }

        for index in 1..EP_COUNT {
            if int_in.epin(index - 1) {
                EP_OUT_WAKERS[index].wake();
            }
            if int_out.epout(index - 1) {                
                EP_OUT_WAKERS[index].wake();
            }
        }
        
    }

}

trait Dir {
    fn dir() -> Direction;
}

/// Marker type for the "IN" direction.
pub enum In {}
impl Dir for In {
    fn dir() -> Direction {
        Direction::In
    }
}

/// Marker type for the "OUT" direction.
pub enum Out {}
impl Dir for Out {
    fn dir() -> Direction {
        Direction::Out
    }
}

trait SealedInstance {
    fn regs() -> crate::pac::usb::Usb;
}

/// USB instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + RccPeripheral + 'static {
    /// Interrupt for this USB instance.
    type Interrupt: interrupt::typelevel::Interrupt;
}

// Internal PHY pins
pin_trait!(DpPin, Instance);
pin_trait!(DmPin, Instance);