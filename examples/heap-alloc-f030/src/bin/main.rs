#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use py32_hal::rcc::{HsiFs, Pll, PllSource, Sysclk};
use {defmt_rtt as _, panic_probe as _};

extern crate alloc;
// use embedded_alloc::TlsfHeap as Heap;
use embedded_alloc::LlffHeap as Heap;
#[global_allocator]
static HEAP: Heap = Heap::empty();

use alloc::boxed::Box;
use alloc::string::*;
use alloc::vec::*;

#[derive(Debug, defmt::Format)]
struct Foo {
    id: u32,
    name: String,
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Hello world!");
    let mut cfg: py32_hal::Config = Default::default();
    cfg.rcc.hsi = Some(HsiFs::HSI_24MHZ);
    cfg.rcc.pll = Some(Pll {
        src: PllSource::HSI,
    });
    cfg.rcc.sys = Sysclk::PLL;
    let _p = py32_hal::init(cfg);

    {
        use core::mem::MaybeUninit;
        use core::ptr::addr_of_mut;
        const HEAP_SIZE: usize = 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(addr_of_mut!(HEAP_MEM) as usize, HEAP_SIZE) }
    };

    let mut xs = Vec::new();
    xs.push("py32-rs team".to_string());
    xs.push("py32f030".to_string());
    xs.push("heap alloc test".to_string());

    info!("vec: {:?}", xs);

    let foo = Box::new(Foo {
        id: 64,
        name: "py32-heap-alloc".to_string(),
    });

    info!("Allocated Box<Foo>: {:?}", foo);

    loop {}
}
