# `usb-device` (`usbd`) Demo

> [!WARNING]
> 
> This demo is intended to showcase the [usb-device](https://crates.io/crates/usb-device) crate, not the [embassy-usb](https://crates.io/crates/embassy-usb) crate.
> 
> If you're looking for examples of USB async drivers, please refer to the [py32f072 examples](https://chatgpt.com/py32f072/src/bin).

## Usage

You need to enable the `usb-device-impl` feature and disable the `embassy-usb-driver-impl` feature.

The PY32 uses a stripped-down version of the MUSB IP. For more information, check out the [musb](https://crates.io/crates/musb) crate. It includes implementations of both [embassy-usb-driver](https://crates.io/crates/embassy-usb-driver) and [usb-device](https://crates.io/crates/usb-device).