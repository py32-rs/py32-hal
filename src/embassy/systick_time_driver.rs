use core::cell::{Cell, RefCell};
use core::task::Waker;

use cortex_m::peripheral::syst::SystClkSource;
use cortex_m::peripheral::SYST;
use cortex_m_rt::exception;

use critical_section::CriticalSection;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time_driver::{Driver, TICK_HZ};
use embassy_time_queue_utils::Queue;
use portable_atomic::{AtomicU64, Ordering};

// Alarm state structure to manage individual alarms
struct AlarmState {
    timestamp: Cell<u64>,
}

unsafe impl Send for AlarmState {}

impl AlarmState {
    const fn new() -> Self {
        Self {
            timestamp: Cell::new(u64::MAX),
        }
    }
}

// SysTick-based time driver implementation
pub(crate) struct SysTickDriver {
    // Total number of ticks since system start
    ticks: AtomicU64,
    // Number of allocated alarms
    alarm: Mutex<CriticalSectionRawMutex, AlarmState>,
    queue: Mutex<CriticalSectionRawMutex, RefCell<Queue>>,
}

// Constant initialization for alarm states
#[allow(clippy::declare_interior_mutable_const)]
const ALARM_STATE_NEW: AlarmState = AlarmState::new();

// Macro to create a static driver instance
embassy_time_driver::time_driver_impl!(static DRIVER: SysTickDriver = SysTickDriver {
    ticks: AtomicU64::new(0),
    alarm: Mutex::const_new(CriticalSectionRawMutex::new(), ALARM_STATE_NEW),
    queue: Mutex::new(RefCell::new(Queue::new()))
});

impl SysTickDriver {
    // Initialize the SysTick driver
    fn init(&'static self, _cs: CriticalSection, mut systick: SYST) -> bool {
        // Calculate the reload value
        let core_clock = unsafe { crate::rcc::get_freqs() }
            .hclk1
            .to_hertz()
            .unwrap()
            .0;

        let reload_value = match (core_clock as u64).checked_div(TICK_HZ) {
            Some(div) if div > 0 && div <= 0x00FFFFFF => (div - 1) as u32,
            _ => panic!("Invalid SysTick reload value"), // Frequency not achievable
        };
        // let peripherals = unsafe { cortex_m::Peripherals::steal() };
        // let mut systick = peripherals.SYST;

        // Configure SysTick
        systick.set_clock_source(SystClkSource::Core); // Use processor clock
        systick.set_reload(reload_value);
        systick.clear_current();
        systick.enable_counter();
        systick.enable_interrupt();

        true
    }

    // SysTick interrupt handler
    fn on_systick(&self) {
        critical_section::with(|cs| {
            // Increment global tick counter
            let current_ticks = self.ticks.fetch_add(1, Ordering::Relaxed);
            self.check_and_trigger_alarm(current_ticks, cs);
        });
    }

    // Check if an alarm is due and trigger it if necessary
    #[inline]
    fn check_and_trigger_alarm(&self, current_time: u64, cs: CriticalSection) {
        let alarm = &self.alarm.borrow(cs);
        let alarm_timestamp = alarm.timestamp.get();

        // Check if alarm is scheduled and due
        if alarm_timestamp != u64::MAX && current_time >= alarm_timestamp {
            let mut next = self
                .queue
                .borrow(cs)
                .borrow_mut()
                .next_expiration(current_time);
            while !self.set_alarm(cs, next) {
                next = self
                    .queue
                    .borrow(cs)
                    .borrow_mut()
                    .next_expiration(self.now());
            }
        }
    }

    // Set alarm timestamp
    fn set_alarm(&self, cs: CriticalSection, timestamp: u64) -> bool {
        if self.now() >= timestamp {
            // Alarm time has passed, cannot set
            return false;
        }
        self.alarm.borrow(cs).timestamp.set(timestamp);
        if self.now() >= timestamp {
            self.alarm.borrow(cs).timestamp.set(u64::MAX);
            return false;
        }
        true
    }
}

// Implement the Driver trait for SysTickDriver
impl Driver for SysTickDriver {
    // Get current system time in ticks
    fn now(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();

            if queue.schedule_wake(at, waker) {
                let mut next = queue.next_expiration(self.now());
                while !self.set_alarm(cs, next) {
                    next = queue.next_expiration(self.now());
                }
            }
        })
    }
}

// Initialization function
pub(crate) fn init(cs: CriticalSection, systick: SYST) {
    DRIVER.init(cs, systick);
}

// SysTick interrupt handler (to be implemented in your interrupt vector)
#[exception]
fn SysTick() {
    DRIVER.on_systick();
}
