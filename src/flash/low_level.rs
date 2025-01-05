use core::ptr::write_volatile;
use core::sync::atomic::{fence, Ordering};

use crate::pac;

use super::{Error, FlashUnit};
use super::values::*;

pub(crate) unsafe fn lock() {
    pac::FLASH.cr().modify(|w| w.set_lock(true));
}

pub(crate) unsafe fn unlock() {
    if pac::FLASH.cr().read().lock() {
        pac::FLASH.keyr().write_value(0x4567_0123);
        pac::FLASH.keyr().write_value(0xCDEF_89AB);
    }
}

// unsafe fn timing_sequence_config() {
//     let hsifs = pac::RCC.icscr().read().hsi_fs();

//     let timing = get_timing_sequence(hsifs);

//     pac::FLASH.ts0().write(|w| w.set_ts0(timing.ts0));
//     pac::FLASH.ts1().write(|w| w.set_ts1(timing.ts1));
//     pac::FLASH.ts3().write(|w| w.set_ts3(timing.ts3));
//     pac::FLASH.ts2p().write(|w| w.set_ts2p(timing.ts2p));
//     pac::FLASH.tps3().write(|w| w.set_tps3(timing.tps3));
//     pac::FLASH.pertpe().write(|w| w.set_pertpe(timing.pertpe));
//     pac::FLASH.smertpe().write(|w| w.set_smertpe(timing.smertpe));
//     pac::FLASH.prgtpe().write(|w| w.set_prgtpe(timing.prgtpe));
//     pac::FLASH.pretpe().write(|w| w.set_pretpe(timing.pretpe));

// }

struct Timing {
    ts0: u8,
    ts1: u16,
    ts3: u8,
    ts2p: u8,
    tps3: u16,
    pertpe: u32,
    smertpe: u32,
    prgtpe: u16,
    pretpe: u16,
}

pub(crate) unsafe fn enable_blocking_write() {
    pac::FLASH.cr().modify(|w| w.set_pg(true));
    pac::FLASH.cr().modify(|w| w.set_eopie(true));
}

pub(crate) unsafe fn disable_blocking_write() {
    pac::FLASH.cr().modify(|w| w.set_pg(false));
}

pub(crate) unsafe fn blocking_write(start_address: u32, buf: &[u8; WRITE_SIZE]) -> Result<(), Error> {
    wait_ready_blocking()?;

    let mut address = start_address;
    for (idx, val) in buf.chunks(4).enumerate() {
        if idx == 63 {
            fence(Ordering::SeqCst);
            pac::FLASH.cr().modify(|w| w.set_pgstrt(true));
        }

        write_volatile(address as *mut u32, u32::from_le_bytes(unwrap!(val.try_into())));
        address += val.len() as u32;

        // prevents parallelism errors
        fence(Ordering::SeqCst);
    }
    wait_ready_blocking()?;
    
    if !pac::FLASH.sr().read().eop() {
        trace!("FLASH: EOP not set");
        info!("FLASH SR.wrperr: {}", pac::FLASH.sr().read().wrperr());
        Err(Error::Prog)
    } else {
        pac::FLASH.sr().modify(|w| w.set_eop(true));
        Ok(())
    }
}

unsafe fn wait_ready_blocking() -> Result<(), Error> {
    loop {
        let sr = pac::FLASH.sr().read();

        if !sr.bsy() {
            if sr.wrperr() {
                return Err(Error::Protected);
            }

            return Ok(());
        }
    }
}

pub(crate) unsafe fn blocking_erase_unit(unit: &FlashUnit) -> Result<(), Error> {
    wait_ready_blocking()?;
    pac::FLASH.cr().modify(|w| {
        match unit {
            FlashUnit::Page(_) => {
                w.set_per(true);
            }
            FlashUnit::Sector(_) => {
                w.set_ser(true);
            }
        }
        w.set_eopie(true);
    });
    match unit {
        FlashUnit::Page(page) => {
            write_volatile(page.start as *mut u32, 0xFFFFFFFF);
        }
        FlashUnit::Sector(sector) => {
            write_volatile(sector.start as *mut u32, 0xFFFFFFFF);
        }
    }
    
    wait_ready_blocking()?;

    if !pac::FLASH.sr().read().eop() {
        trace!("FLASH: EOP not set");
        Err(Error::Prog)
    } else {
        pac::FLASH.sr().modify(|w| w.set_eop(true));
        Ok(())
    }?;
    
    pac::FLASH.cr().modify(|w| {
        match unit {
            FlashUnit::Page(_) => {
                w.set_per(false);
            }
            FlashUnit::Sector(_) => {
                w.set_ser(false);
            }
        }
    });
    clear_all_err();
    Ok(())
}

pub(crate) unsafe fn clear_all_err() {
    // read and write back the same value.
    // This clears all "write 1 to clear" bits.
    pac::FLASH.sr().modify(|_| {});
}