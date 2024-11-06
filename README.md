# py32-hal

> **Note**
> This project is under development. While it's usable for experimentation and testing,
> it may not be fully stable for production environments.
> We welcome user feedback and encourage reporting any issues you encounter to help improve the hal crate.

This HAL crates is the [Embassy](https://github.com/embassy-rs/embassy) framework driver for Puya microcontrollers.

This HAL crates uses the metapac approach to support multiple chips in the same crate.

The metapac is maintained in the [py32-rs/py32-data](https://github.com/py32-rs/py32-data) repository, published as a crate `py32-metapac`.

Keypoints:

- Embassy support
- All-in-one metapac for peripheral register access, check [py32-data](https://github.com/py32-rs/py32-data) for more
- All-in-one HAL crate, no need to create a new crate for each chip
- Async drivers, with async/await support, DMA (TODO)support
- Write once, run on all supported chips(should be)

## Supported Devices and Peripherals

Currently, supported chips are listed in `Cargo.toml` as feature flags.

Supported chip flags: py32f030f16, py32f030k28, More is coming...

others should work if you are careful as most peripherals are similar enough.In fact, the IPs of peripherals in different PY32 series may be consistent. Moreover, some series use the same die, so it might not require much work.

For a full list of chip capabilities and peripherals, check the [py32-data](https://github.com/py32-rs/py32-data) repository.

| Family     | F002B/L020/F001 | F030/F003/F002A | F040/F07x/MD410 | F403 |
| ---------- | --------------- | --------------- | --------------- | ---- |
| Embassy    |                 | ✅               |                 |      |
| RCC        |                 | ✅               |                 |      |
| GPIO       |                 | ✅               |                 |      |
| INTERRUPT  |                 | ✅               |                 |      |
| DMA        | N/A             |                 |                 |      |
| EXTI       |                 |                 |                 |      |
| USART      |                 |                 |                 |      |
| I2C*       |                 |                 |                 |      |
| SPI*       |                 |                 |                 |      |
| ADC*       |                 | ✅               |                 |      |
| RTC        |                 |                 |                 |      |
| Timer(PWM) |                 | ✅               |                 |      |
| USB/OTG    |                 |                 | N/A             | N/A  |

- ✅ : Expected to work
- ❌ : Not implemented
- ❓ : Not tested
- `*` marks the async driver
- TODO: I haven't got a dev board yet, help-wanted
- N/A: Not available

### TODOs

Too many...

## Minimum supported Rust version(MSRV)

This project is developed with a recent **nightly** version of Rust compiler. And is expected to work with beta versions of Rust.

Feel free to change this if you did some testing with some version of Rust.

## Contributing

All kinds of contributions are welcome.

- Share your project at [Discussions](https://github.com/py32-rs/py32-hal/discussions)
  - if your project is an open-source project, consider adding it to the awesome list (TODO)
- Support new MCUs.
- README and Documentation, including doc comments in code
- Writing demo code for peripherals
- Revising the peripheral definitions at [py32-data](https://github.com/py32-rs/py32-data)
- Adding new peripheral drivers
- ...

## License

This project is licensed under the MIT or Apache-2.0 license, at your option.