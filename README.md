# py32-hal

[![Crates.io][badge-license]][crates]
[![Crates.io][badge-version]][crates]
[![docs.rs][badge-docsrs]][docsrs]

[badge-license]: https://img.shields.io/crates/l/py32-hal?style=for-the-badge
[badge-version]: https://img.shields.io/crates/v/py32-hal?style=for-the-badge
[badge-docsrs]: https://img.shields.io/docsrs/py32-hal?style=for-the-badge
[crates]: https://crates.io/crates/py32-hal
[docsrs]: https://docs.rs/py32-hal

> **Note**
> This project is under development. While it's usable for experimentation and testing,
> it may not be fully stable for production environments.
> We welcome user feedback and encourage reporting any issues you encounter to help improve the hal crate.

A Rust HAL and [Embassy](https://embassy.dev/) driver implementation for Puya py32 MCUs.

This HAL crates uses the metapac approach to support multiple chips in the same crate. The metapac is maintained in the [py32-rs/py32-data](https://github.com/py32-rs/py32-data) repository, published as a crate `py32-metapac`.

Keypoints:

- Embassy support
- All-in-one metapac for peripheral register access, check [py32-data](https://github.com/py32-rs/py32-data) for more
- All-in-one HAL crate, no need to create a new crate for each chip
- Async drivers, with async/await support, DMA(TODO) support
- Write once, run on all supported chips(should be)

## Supported Devices and Peripherals

The currently supported chips are listed as feature flags in `Cargo.toml`:  
- `py32f002b`  
- `py32f030f16`  
- `py32f030k28`  
- `py32f072c1b`  

(More will be added soon!)

The current chip features are independent of packaging. We haven't implemented separate handling for cut-down series yet. Please check the [examples‘ readme](examples/README.md) for compatibility information.

 Chips outside the list may work if you proceed cautiously, as most peripherals are quite similar across the series. In fact, the peripheral IPs within different PY32 series are often consistent. Additionally, some series share the same die, which may minimize the effort required for compatibility.  

For a comprehensive list of chip capabilities and peripherals, refer to the [py32-data](https://github.com/py32-rs/py32-data) repository and [examples‘ readme](examples/README.md).

| Family      | F002B/L020 | F030/F003/F002A | F040/F07x/MD410 | F403 |
| ----------- | ---------- | --------------- | --------------- | ---- |
| Embassy     | ✅+         | ✅+              | ✅+              |      |
| RCC         | ✅          | ✅               | ✅               |      |
| GPIO & EXTI | ✅+         | ✅+              | ✅+              |      |
| INTERRUPT   | ✅          | ✅               | ✅               |      |
| DMA         | N/A        | ✅+              | ✅+              |      |
| USART       | ✅          | ✅+              | ✅+              |      |
| I2C         | ❓          | ✅+              | ✅+              |      |
| SPI         |            |                 |                 |      |
| ADC         | ✅+         | ✅+              | ✅               |      |
| RTC         | ❓          |                 |                 |      |
| FLASH       | ❓          | ✅               | ✅               |      |
| Timer(PWM)  | ❓          | ✅               | ❓               |      |
| USB         | N/A        | N/A             | ✅+              |      |
| DAC         | N/A        | N/A             |                 |      |
| I2S         | N/A        | N/A             |                 |      |

- ✅ : Implemented
- Blank : Not implemented
- ❓ : Requires demo verification
- `+` : Async support
- N/A : Not available

### time-driver

This crate provides an implementation of the Embassy `time-driver`.

Embassy requires that any TIM used as a time-driver has at least two channels, so only TIM1 and TIM3 are available for the PY32F030, 003, and 002A series. You can select either `time-driver-tim3` or `time-driver-tim1` to specify the TIM to use.

For PY32F07x, F040, you can use TIM15, TIM3, TIM2 or TIM1.

`time-driver-systick`: Although we do not recommend using it and there are some shortcomings, it does work. For details, please see [systick-demo](examples/systick-time-driver-f030/README.md)

### Feature: `unsafe-reuse-swd-pins`

This feature is **disabled by default** for all chip series.

> **WARNING:** This is a dangerous feature. Enable it only if you fully understand the risks and have a recovery plan.

This feature allows the SWD debug pins to be reused for other purposes (like GPIO). Now py32-hal does not have a built-in power-on delay like C HAL, so reconfiguring the SWD pins immediately on startup can permanently lock you out from reprogramming or debugging the chip, effectively 'bricking' it.

The risk is critical on MCUs that lack a BOOT button and a factory bootloader, such as the `PY32F002B`, `PY32L020`, and `PY32C64x` series (SWD on pins like `PA2` and `PB6`).

## Awesome List

[py32csdk-hal-sys](https://github.com/decaday/py32csdk-hal-sys): PY32F0 MCU c SDK bindings rust crate

## TODOs

Too many...

- Test F072 peripherals

- HSE test and examples

- Other series, chips

- SPI, RTC

- F072 TIM2(GP32) support

- ...

## Contributing

All kinds of contributions are welcome.

- Share your project at [Discussions](https://github.com/py32-rs/py32-hal/discussions)
  - if your project is an open-source project, consider adding it to the awesome list
- Support new MCUs
- README and Documentation, including doc comments in code
- Writing demo code for peripherals
- Revising the peripheral definitions at [py32-data](https://github.com/py32-rs/py32-data)
- Adding new peripheral drivers
- ...

## Minimum supported Rust version(MSRV)

This project is developed with a recent **nightly** version of Rust compiler. And is expected to work with beta versions of Rust.

Feel free to change this if you did some testing with some version of Rust.

## License

This project is licensed under the MIT or Apache-2.0 license, at your option.

Some peripheral driver code has been modified from [embassy-stm32](https://github.com/embassy-rs/embassy/tree/main/embassy-stm32). Big thanks to this project and its awesome contributors!
