# Examples for py32-hal

This directory contains various examples demonstrating the usage of `py32-hal` with different MCU series and features.

## py32f002b
Examples compatible with the following MCU series:
- PY32F002B
- PY32L020
- PY32C642/C641

## py32f030
Examples compatible with the following MCU series:
- PY32F030
- PY32F003
- PY32F002A
- PY32C613
- PY32M030

## py32f072
Examples compatible with the following MCU series:
- PY32F072
- PY32F071
- PY32F040
- PY32F031
- PY32M070
- PY32MD410

## heap-alloc-f030
Demonstrates heap allocation using the [embedded-alloc](https://github.com/rust-embedded/embedded-alloc) crate with either LLFF or TLSF heap implementations.

This example is based on py32f030 but can be easily adapted for other series.

## usbd-f072
USB demonstration using the [usb-device](https://github.com/rust-embedded-community/usb-device) crate. If you're interested in using an async USB stack, check out the [embassy-usb](https://crates.io/crates/embassy-usb) examples in the [py32f072](py32f072) directory.

This example is designed for py32f072.

## systick-time-driver-f030
Demonstrates using SysTick as a time driver (though this approach is not recommended for production use).

This example is based on py32f030 but can be easily adapted for other series.



# Contributing

Feel free to contribute new examples or improve existing ones by submitting pull requests.
