use core::ptr::write_volatile;
use core::sync::atomic::{fence, Ordering};

use crate::pac;
use crate::pac::rcc::vals::HsiFs;

use super::values::*;
use super::{Error, FlashUnit};

pub(crate) unsafe fn lock() {
    pac::FLASH.cr().modify(|w| w.set_lock(true));
}

pub(crate) unsafe fn unlock() {
    if pac::FLASH.cr().read().lock() {
        pac::FLASH.keyr().write_value(0x4567_0123);
        pac::FLASH.keyr().write_value(0xCDEF_89AB);
    }
}

pub(crate) unsafe fn enable_blocking_write() {
    pac::FLASH.cr().modify(|w| w.set_pg(true));
    pac::FLASH.cr().modify(|w| w.set_eopie(true));
}

pub(crate) unsafe fn disable_blocking_write() {
    pac::FLASH.cr().modify(|w| w.set_pg(false));
}

pub(crate) unsafe fn blocking_write(
    start_address: u32,
    buf: &[u8; WRITE_SIZE],
) -> Result<(), Error> {
    wait_ready_blocking()?;

    let mut address = start_address;
    for (idx, val) in buf.chunks(4).enumerate() {
        if idx == PAGE_SIZE / 4 - 1 {
            fence(Ordering::SeqCst);
            pac::FLASH.cr().modify(|w| w.set_pgstrt(true));
        }

        write_volatile(
            address as *mut u32,
            u32::from_le_bytes(unwrap!(val.try_into())),
        );
        address += val.len() as u32;

        // prevents parallelism errors
        fence(Ordering::SeqCst);
    }
    wait_ready_blocking()?;

    if !pac::FLASH.sr().read().eop() {
        trace!("FLASH: EOP not set");
        trace!("FLASH SR.wrperr: {}", pac::FLASH.sr().read().wrperr());
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

    pac::FLASH.cr().modify(|w| match unit {
        FlashUnit::Page(_) => {
            w.set_per(false);
        }
        FlashUnit::Sector(_) => {
            w.set_ser(false);
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

pub(crate) unsafe fn timing_sequence_config(configured: Option<HsiFs>) {
    let hsifs = pac::RCC.icscr().read().hsi_fs();

    if Some(hsifs) != configured {
        #[cfg(py32f002b)]
        let eppara = pac::CONFIGBYTES.eppara();
        #[cfg(not(py32f002b))]
        let eppara = pac::CONFIGBYTES.eppara(hsifs as usize);

        pac::FLASH
            .ts0()
            .write(|w| w.set_ts0(eppara.eppara0().read().ts0()));
        pac::FLASH
            .ts1()
            .write(|w| w.set_ts1(eppara.eppara0().read().ts1()));
        pac::FLASH
            .ts3()
            .write(|w| w.set_ts3(eppara.eppara0().read().ts3()));
        pac::FLASH
            .ts2p()
            .write(|w| w.set_ts2p(eppara.eppara1().read().ts2p()));
        pac::FLASH
            .tps3()
            .write(|w| w.set_tps3(eppara.eppara1().read().tps3()));
        pac::FLASH
            .pertpe()
            .write(|w| w.set_pertpe(eppara.eppara2().read().pertpe()));
        pac::FLASH
            .smertpe()
            .write(|w| w.set_smertpe(eppara.eppara3().read().smertpe()));
        pac::FLASH
            .pretpe()
            .write(|w| w.set_pretpe(eppara.eppara4().read().pretpe()));
        pac::FLASH
            .prgtpe()
            .write(|w| w.set_prgtpe(eppara.eppara4().read().prgtpe()));
    }
}
