## Systick Time-Driver Demo

Although we do not recommend using it and there are some shortcomings, it does work (after all, some chips lack sufficient multi-channel timers).

Currently, the `systick-time-driver` implementation in `py32-hal` uses a retry loop. The reload value register (RVR) of the Systick timer is set to a fixed value, causing high-frequency interrupts. During these interrupts, it updates the tick count and checks whether any alarm has expired (since Systick lacks compare-capture functionality).

The interrupt frequency of Systick matches the tick frequency configured for `embassy-time`. It is **not recommended** to use excessively high frequencies  because `time-driver-systick` generates frequent interrupts, unlike `time-driver-timx`. Additionally, `time-driver-systick` updates the tick count and wakes up alarms in software, which takes longer.

### Usage

Enable the `time-driver-systick` feature. Then, before initializing `py32-hal`, add the following:

```rust
use cortex_m::Peripherals;
let cp = Peripherals::take().unwrap();
let systick = cp.SYST;
```

Pass the `systick` instance during initialization:

```rust
let p = py32_hal::init(Default::default(), systick);
```

Complete code demo can be found in the same directory as this documentation.

### Feature: `td-systick-multi-alarms`

By default, only one alarm is provided (similar to a 2-channel timer). Enabling this feature provides three alarms (similar to a 4-channel timer).

Of course, this will also increase the execution time of the interrupt handler.

### More

[SysTick time driver · Issue #786 · embassy-rs/embassy](https://github.com/embassy-rs/embassy/issues/786)

Here are the key disadvantages of using **SysTick** as a time-driver:

- **Clock Source Uncertainty**: SysTick supports two clock sources, but their speeds are not known at compile time, requiring vendor-specific runtime translation.
- **Interrupt Limitation**: SysTick can only trigger an interrupt at zero, complicating setting alarms for specific points in the cycle.
- **Dual-Core Challenges**: On dual-core devices like the RP2040, each core has an independent SysTick timer with no cross-access.
- **Perturbation Issues**: Modifying the counter register while counting can perturb the overall tick count, affecting timing accuracy.