pub use crate::pac::rcc::vals::{
    Hpre as AHBPrescaler, HsiFs, Hsidiv, Ppre as APBPrescaler, Sw as Sysclk,
};

use crate::pac::{CONFIGBYTES, FLASH, RCC};
use crate::time::Hertz;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Hse {
    /// HSE frequency.
    pub freq: Hertz,
}

/// Clocks configutation
#[non_exhaustive]
#[derive(Clone, Copy)]
pub struct Config {
    pub hsi: Option<HsiFs>,
    pub hsidiv: Hsidiv,
    pub hse: Option<Hse>,
    pub sys: Sysclk,

    pub ahb_pre: AHBPrescaler,
    pub apb1_pre: APBPrescaler,
    /// Per-peripheral kernel clock selection muxes
    pub mux: super::mux::ClockMux,
    // pub ls: super::LsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hsi: Some(HsiFs::HSI_24MHZ),
            hse: None,
            sys: Sysclk::HSI,
            hsidiv: Hsidiv::DIV1,
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
    let hsi_value = if let Some(value) = config.hsi {
        let hsi_trimming_bytes = CONFIGBYTES.hsi_trimming().read();

        assert_eq!(hsi_trimming_bytes.hsi_fs(), value as u8);

        RCC.icscr().modify(|w| {
            w.set_hsi_fs(value);
            w.set_hsi_trim(hsi_trimming_bytes.hsi_trim());
        });

        match value {
            HsiFs::HSI_24MHZ => Some(Hertz(24_000_000)),
            _ => unreachable!(),
        }
    } else {
        None
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
            RCC.cr().modify(|w| w.set_hseen(false));
            None
        }
        Some(hse) => {
            RCC.cr().modify(|w| w.set_hseen(true));
            Some(hse.freq)
        }
    };

    // Configure sysclk
    let sys = match config.sys {
        Sysclk::HSI => unwrap!(hsi_value) / config.hsidiv,
        Sysclk::HSE => unwrap!(hse),
        _ => unreachable!(),
    };

    let hclk1 = sys / config.ahb_pre;
    let (pclk1, pclk1_tim) = super::util::calc_pclk(hclk1, config.apb1_pre);

    let latency: u8 = match hclk1.0 {
        ..=24_000_000 => 0,
        _ => 1,
    };
    FLASH.acr().modify(|w| {
        w.set_latency(latency != 0);
    });
    // Set prescalers
    RCC.cfgr().modify(|w| {
        w.set_ppre(config.apb1_pre);
        w.set_hpre(config.ahb_pre);
    });

    // Wait for the new prescalers to kick in
    // "The clocks are divided with the new prescaler factor from
    //  1 to 16 AHB cycles after write"
    cortex_m::asm::delay(16);

    // CFGR has been written before (clock divider) don't overwrite these settings
    RCC.cfgr().modify(|w| w.set_sw(config.sys));
    while RCC.cfgr().read().sws() != config.sys {}

    // Disable HSI if not used
    if hsi == None {
        RCC.cr().modify(|w| w.set_hsion(false));
    }

    // let rtc = config.ls.init();

    config.mux.init();

    let clocks = crate::rcc::Clocks {
        hclk1: Some(hclk1).into(),
        pclk1: Some(pclk1).into(),
        pclk1_tim: Some(pclk1_tim).into(),
        sys: Some(sys).into(),
        hsi: hsi_value.into(),
        lse: None.into(),
    };
    crate::rcc::set_freqs(clocks);
}
