// use crate::pac::flash::vals::Latency;
use crate::pac::rcc::vals::Pllsrc;
// pub use crate::pac::rcc::vals::Prediv as PllPreDiv;
#[cfg(rcc_f072)]
pub use crate::pac::rcc::vals::Pllmul as PllMul;
pub use crate::pac::rcc::vals::{
    Hpre as AHBPrescaler, HsiFs, Hsidiv, Ppre as APBPrescaler, Sw as Sysclk,
};

use crate::pac::{/* FLASH , */ RCC};
use crate::time::Hertz;

// /// HSI speed
// pub const HSI_FREQ: Hertz = Hertz(8_000_000);

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum HseMode {
    /// crystal/ceramic oscillator (HSEBYP=0)
    Oscillator,
    /// external analog clock (low swing) (HSEBYP=1)
    Bypass,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Hse {
    /// HSE frequency.
    pub freq: Hertz,
    /// HSE mode.
    pub mode: HseMode,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Hsi {
    /// HSE frequency.
    pub freq: Hertz,
    /// HSE mode.
    pub mode: HseMode,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum PllSource {
    HSE,
    HSI,
}

#[derive(Clone, Copy)]
pub struct Pll {
    pub src: PllSource,

    // /// PLL pre-divider.
    // ///
    // /// On some chips, this must be 2 if `src == HSI`. Init will panic if this is not the case.
    // pub prediv: PllPreDiv,
    #[cfg(rcc_f072)]
    /// PLL multiplication factor.
    pub mul: PllMul,
}

/// Clocks configutation
#[non_exhaustive]
#[derive(Clone, Copy)]
pub struct Config {
    pub hsi: Option<Hertz>,
    pub hsidiv: Hsidiv,
    pub hse: Option<Hse>,
    pub sys: Sysclk,

    pub pll: Option<Pll>,

    pub ahb_pre: AHBPrescaler,
    pub apb1_pre: APBPrescaler,
    /// Per-peripheral kernel clock selection muxes
    pub mux: super::mux::ClockMux,
    // pub ls: super::LsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hsi: Some(Hertz::mhz(8)),
            hse: None,
            sys: Sysclk::HSI,
            hsidiv: Hsidiv::DIV1,
            pll: None,
            ahb_pre: AHBPrescaler::DIV1,
            apb1_pre: APBPrescaler::DIV1,
            // ls: Default::default(),
            mux: Default::default(),
        }
    }
}

/// Initialize and Set the clock frequencies
pub(crate) unsafe fn init(config: Config) {
    // Turn on the HSI
    RCC.cr().modify(|w| w.set_hsion(true));
    if let Some(value) = config.hsi {
        let val = value.0;

        #[cfg(rcc_f030)]
        let (fs_val, trim_addr): (HsiFs, usize) = match val {
            4_000_000u32 => (HsiFs::HSI_4MHZ, 0x1FFF_0F00),
            8_000_000u32 => (HsiFs::HSI_8MHZ, 0x1FFF_0F04),
            16_000_000u32 => (HsiFs::HSI_16MHZ, 0x1FFF_0F08),
            22_120_000u32 => (HsiFs::HSI_22_12MHZ, 0x1FFF_0F0C),
            24_000_000u32 => (HsiFs::HSI_24MHZ, 0x1FFF_0F10),
            _ => panic!("Unsupported HSI frequency"),
        };

        #[cfg(rcc_f072)]
        let (fs_val, trim_addr): (HsiFs, usize) = match val {
            4_000_000u32 => (HsiFs::HSI_4MHZ, 0x1FFF_3200),
            8_000_000u32 => (HsiFs::HSI_8MHZ, 0x1FFF_3208),
            16_000_000u32 => (HsiFs::HSI_16MHZ, 0x1FFF_3210),
            22_120_000u32 => (HsiFs::HSI_22_12MHZ, 0x1FFF_3218),
            24_000_000u32 => (HsiFs::HSI_24MHZ, 0x1FFF_3220),
            _ => panic!("Unsupported HSI frequency"),
        };
        let trim_val = (unsafe { *(trim_addr as *const u32) } & 0x1FFF) as u16;
        RCC.icscr().modify(|w| {
            w.set_hsi_fs(fs_val);
            w.set_hsi_trim(trim_val);
        });
    };
    while !RCC.cr().read().hsirdy() {}

    // Use the HSI clock as system clock during the actual clock setup
    RCC.cfgr().modify(|w| w.set_sw(Sysclk::HSI));
    while RCC.cfgr().read().sws() != Sysclk::HSI {}

    RCC.cr().modify(|w| w.set_hsidiv(config.hsidiv));

    // Configure HSI
    let hsi = config.hsi;

    // Configure HSE
    let hse = match config.hse {
        None => {
            RCC.cr().modify(|w| w.set_hseon(false));
            None
        }
        Some(hse) => {
            match hse.mode {
                HseMode::Bypass => assert!(max::HSE_BYP.contains(&hse.freq)),
                HseMode::Oscillator => assert!(max::HSE_OSC.contains(&hse.freq)),
            }

            RCC.cr()
                .modify(|w| w.set_hsebyp(hse.mode != HseMode::Oscillator));
            RCC.cr().modify(|w| w.set_hseon(true));
            while !RCC.cr().read().hserdy() {}
            Some(hse.freq)
        }
    };
    // Configure PLL
    let pll = match config.pll {
        None => None,
        Some(pll) => {
            let (src_val, src_freq) = match pll.src {
                PllSource::HSE => (Pllsrc::HSE, unwrap!(hse)),
                PllSource::HSI => (Pllsrc::HSI, unwrap!(hsi)),
            };
            #[cfg(rcc_f030)]
            let out_freq = src_freq * 2u8;
            #[cfg(rcc_f072)]
            let out_freq = src_freq * pll.mul;
            assert!(max::PLL_IN.contains(&src_freq));
            // assert!(max::PLL_OUT.contains(&pll.src.out_freq(pll.mul)));

            RCC.cr().modify(|w| w.set_pllon(false));
            while RCC.cr().read().pllrdy() {}

            RCC.pllcfgr().modify(|w| {
                #[cfg(rcc_f072)]
                w.set_pllmul(pll.mul);
                w.set_pllsrc(src_val);
            });
            RCC.cr().modify(|w| w.set_pllon(true));
            cortex_m::asm::delay(1_000);
            while !RCC.cr().read().pllrdy() {}
            Some(out_freq)
        }
    };

    // let usb = match pll {
    //     Some(Hertz(72_000_000)) => Some(crate::pac::rcc::vals::Usbpre::DIV1_5),
    //     Some(Hertz(48_000_000)) => Some(crate::pac::rcc::vals::Usbpre::DIV1),
    //     _ => None,
    // }
    // .map(|usbpre| {
    //     RCC.cfgr().modify(|w| w.set_usbpre(usbpre));
    //     Hertz(48_000_000)
    // });

    // Configure sysclk
    let sys = match config.sys {
        Sysclk::HSI => unwrap!(hsi) / config.hsidiv,
        Sysclk::HSE => unwrap!(hse),
        Sysclk::PLL => unwrap!(pll),
        _ => unreachable!(),
    };

    let hclk1 = sys / config.ahb_pre;
    let (pclk1, pclk1_tim) = super::util::calc_pclk(hclk1, config.apb1_pre);

    // assert!(max::HCLK.contains(&hclk));
    // assert!(max::PCLK.contains(&pclk));

    // // Set latency based on HCLK frquency
    // let latency = match hclk.0 {
    //     ..=24_000_000 => Latency::WS0,
    //     ..=48_000_000 => Latency::WS1,
    //     _ => Latency::WS2,
    // };

    // FLASH.acr().modify(|w| {
    //     w.set_latency(latency);
    //     // RM0316: "The prefetch buffer must be kept on when using a prescaler
    //     // different from 1 on the AHB clock.", "Half-cycle access cannot be
    //     // used when there is a prescaler different from 1 on the AHB clock"
    //     if config.ahb_pre != AHBPrescaler::DIV1 {
    //         w.set_hlfcya(false);
    //         w.set_prftbe(true);
    //     }
    // });

    let latency: u32 = match hclk1.0 {
        ..=24_000_000 => 0,
        ..=48_000_000 => 1,
        _ => 2,
    };
    // Temporarily: set flash latency
    unsafe {
        let acr_reg = 0x4002_2000 as *mut u32;
        let value = acr_reg.read_volatile() | latency;
        acr_reg.write_volatile(value);
    }

    // Set prescalers
    // CFGR has been written before (PLL, PLL48) don't overwrite these settings
    RCC.cfgr().modify(|w| {
        w.set_ppre(config.apb1_pre);
        w.set_hpre(config.ahb_pre);
    });

    // Wait for the new prescalers to kick in
    // "The clocks are divided with the new prescaler factor from
    //  1 to 16 AHB cycles after write"
    cortex_m::asm::delay(16);

    // CFGR has been written before (PLL, PLL48, clock divider) don't overwrite these settings
    RCC.cfgr().modify(|w| w.set_sw(config.sys));
    while RCC.cfgr().read().sws() != config.sys {}

    // Disable HSI if not used
    if hsi == None {
        RCC.cr().modify(|w| w.set_hsion(false));
    }

    // let rtc = config.ls.init();

    /*
    TODO: Maybe add something like this to clock_mux? How can we autogenerate the data for this?
    let hrtim = match config.hrtim {
        // Must be configured after the bus is ready, otherwise it won't work
        HrtimClockSource::BusClk => None,
        HrtimClockSource::PllClk => {
            use crate::pac::rcc::vals::Timsw;

            // Make sure that we're using the PLL
            let pll = unwrap!(pll);
            assert!((pclk2 == pll) || (pclk2 * 2u32 == pll));

            RCC.cfgr3().modify(|w| w.set_hrtim1sw(Timsw::PLL1_P));

            Some(pll * 2u32)
        }
    };
     */

    config.mux.init();

    // set_clocks!(
    //     hsi: hsi,
    //     hse: hse,
    //     pll: pll,
    //     sys: Some(sys),
    //     pclk1: Some(pclk1),
    //     pclk1_tim: Some(pclk1_tim),
    //     hclk1: Some(hclk1),
    //     rtc: rtc,
    //     // usb: usb,
    //     lse: None,
    // );

    let clocks = crate::rcc::Clocks {
        hclk1: Some(hclk1).into(),
        pclk1: Some(pclk1).into(),
        pclk1_tim: Some(pclk1_tim).into(),
        sys: Some(sys).into(),
        hsi: hsi.into(),
        lse: None.into(),
        pll: pll.into(),
    };
    crate::rcc::set_freqs(clocks);
}

mod max {
    use core::ops::RangeInclusive;

    use crate::time::Hertz;

    pub(crate) const HSE_OSC: RangeInclusive<Hertz> = Hertz(4_000_000)..=Hertz(32_000_000);
    pub(crate) const HSE_BYP: RangeInclusive<Hertz> = Hertz(1_000_000)..=Hertz(32_000_000);

    // pub(crate) const HCLK: RangeInclusive<Hertz> = Hertz(0)..=Hertz(48_000_000);
    // pub(crate) const PCLK1: RangeInclusive<Hertz> = Hertz(0)..=Hertz(48_000_000);

    #[cfg(any(rcc_f030, rcc_f072))]
    pub(crate) const PLL_IN: RangeInclusive<Hertz> = Hertz(16_000_000)..=Hertz(24_000_000);
    // pub(crate) const PLL_OUT: RangeInclusive<Hertz> = Hertz(16_000_000)..=Hertz(48_000_000);
}
