use core::mem::MaybeUninit;

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub hclk1: Option<crate::time::Hertz>,
    pub pclk1: Option<crate::time::Hertz>,
    pub pclk1_tim: Option<crate::time::Hertz>,
    pub pclk2: Option<crate::time::Hertz>,
    pub pclk2_tim: Option<crate::time::Hertz>,
    //  pub rtc: Option<crate::time::Hertz>,
    // pub sys: Option<crate::time::Hertz>,
    // pub usb: Option<crate::time::Hertz>,
}

static mut CLOCK_FREQS: MaybeUninit<Clocks> = MaybeUninit::uninit();

pub struct Config {}

pub fn init(config: Config) {
    // TODO
    let _ = config;
    unsafe {
        CLOCK_FREQS.as_mut_ptr().write(Clocks {
            hclk1: Some(crate::time::Hertz(8_000_000)),
            pclk1: Some(crate::time::Hertz(8_000_000)),
            pclk1_tim: Some(crate::time::Hertz(8_000_000)),
            pclk2: Some(crate::time::Hertz(8_000_000)),
            pclk2_tim: Some(crate::time::Hertz(8_000_000)),
        });
    }
}

pub(crate) fn clocks() -> Clocks {
    unsafe { CLOCK_FREQS.assume_init() }
}
