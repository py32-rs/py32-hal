use core::cell::Cell;
use core::ptr;

use cortex_m::peripheral::syst::SystClkSource;
use cortex_m::peripheral::SYST;
use cortex_m_rt::exception;

use portable_atomic::{AtomicU32, Ordering};
use critical_section::CriticalSection;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time_driver::{AlarmHandle, Driver, TICK_HZ};

// Maximum number of supported alarms
const ALARM_COUNT: usize = 3;

// Alarm state structure to manage individual alarms
struct AlarmState {
    timestamp: Cell<u64>,
    callback: Cell<*const ()>,
    ctx: Cell<*mut ()>,
}

unsafe impl Send for AlarmState {}

impl AlarmState {
    const fn new() -> Self {
        Self {
            timestamp: Cell::new(u64::MAX),
            callback: Cell::new(ptr::null()),
            ctx: Cell::new(ptr::null_mut()),
        }
    }
}

// SysTick-based time driver implementation
pub(crate) struct SysTickDriver {
    // Total number of ticks since system start
    ticks: AtomicU32,
    // Number of allocated alarms
    alarm_count: AtomicU32,
    // Mutex-protected array of alarms
    alarms: Mutex<CriticalSectionRawMutex, [AlarmState; ALARM_COUNT]>,
}

// Constant initialization for alarm states
#[allow(clippy::declare_interior_mutable_const)]
const ALARM_STATE_NEW: AlarmState = AlarmState::new();

// Macro to create a static driver instance
embassy_time_driver::time_driver_impl!(static DRIVER: SysTickDriver = SysTickDriver {
    ticks: AtomicU32::new(0),
    alarm_count: AtomicU32::new(0),
    alarms: Mutex::const_new(CriticalSectionRawMutex::new(), [ALARM_STATE_NEW; ALARM_COUNT]),
});

impl SysTickDriver {
    // Initialize the SysTick driver
    fn init(&'static self, _cs: CriticalSection, mut systick: SYST) -> bool {
        // Calculate the reload value
        let core_clock = unsafe { crate::rcc::get_freqs() }.hclk1.to_hertz().unwrap().0;

        let reload_value = match (core_clock as u64).checked_div(TICK_HZ) {
            Some(div) if div > 0 && div <= 0x00FFFFFF => (div - 1) as u32,
            _ => panic!("Invalid SysTick reload value"), // Frequency not achievable
        };
        // let peripherals = unsafe { cortex_m::Peripherals::steal() };
        // let mut systick = peripherals.SYST;

        // Configure SysTick
        systick.set_clock_source(SystClkSource::Core);  // Use processor clock
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
            let current_time = current_ticks as u64;

            // Check and trigger any due alarms
            for n in 0..ALARM_COUNT {
                self.check_and_trigger_alarm(n, current_time, cs);
            }
        });
    }

    // Check if an alarm is due and trigger it if necessary
    fn check_and_trigger_alarm(&self, n: usize, current_time: u64, cs: CriticalSection) {
        let alarm = &self.alarms.borrow(cs)[n];
        let alarm_timestamp = alarm.timestamp.get();

        // Check if alarm is scheduled and due
        if alarm_timestamp != u64::MAX && current_time >= alarm_timestamp {
            // Reset timestamp
            alarm.timestamp.set(u64::MAX);

            // Safety: We know the callback is valid when set
            let f: fn(*mut ()) = unsafe { core::mem::transmute(alarm.callback.get()) };
            f(alarm.ctx.get());
        }
    }
}

// Implement the Driver trait for SysTickDriver
impl Driver for SysTickDriver {
    // Get current system time in ticks
    fn now(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed) as u64
    }

    // Allocate a new alarm
    unsafe fn allocate_alarm(&self) -> Option<AlarmHandle> {
        critical_section::with(|_| {
            let id = self.alarm_count.load(Ordering::Relaxed);
            if id < ALARM_COUNT as u32 {
                self.alarm_count.store(id + 1, Ordering::Relaxed);
                Some(AlarmHandle::new(id as u8))
            } else {
                None
            }
        })
    }

    // Set alarm callback
    fn set_alarm_callback(&self, alarm: AlarmHandle, callback: fn(*mut ()), ctx: *mut ()) {
        critical_section::with(|cs| {
            let alarm_state = &self.alarms.borrow(cs)[alarm.id() as usize];
            alarm_state.callback.set(callback as *const ());
            alarm_state.ctx.set(ctx);
        });
    }

    // Set alarm timestamp
    fn set_alarm(&self, alarm: AlarmHandle, timestamp: u64) -> bool {
        critical_section::with(|cs| {
            let n = alarm.id() as usize;
            let alarm_state = &self.alarms.borrow(cs)[n];

            let current_time = self.now();
            if timestamp <= current_time {
                // Alarm time has passed, cannot set
                return false;
            }

            // Set alarm timestamp
            alarm_state.timestamp.set(timestamp);
            true
        })
    }
}

// Initialization function
pub(crate) fn init(cs: CriticalSection, systick: SYST) {
    DRIVER.init(cs, systick);
}

// SysTick interrupt handler (to be implemented in your interrupt vector)
#[exception]
fn SysTick(){
    DRIVER.on_systick();
}