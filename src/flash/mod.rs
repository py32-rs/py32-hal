use core::marker::PhantomData;
use core::sync::atomic::{fence, Ordering};

use embassy_hal_internal::drop::OnDrop;
use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embedded_storage::nor_flash::{NorFlashError, NorFlashErrorKind};

use crate::mode::{Async, Blocking};
use crate::peripherals::FLASH;

mod low_level;

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    Prog,
    Size,
    Miss,
    Seq,
    Protected,
    Unaligned,
    Parallelism,
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct FlashSector {
    pub start: u32,
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct FlashPage {
    pub start: u32,
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FlashUnit {
    Page(FlashPage),
    Sector(FlashSector),
}

/// Internal flash memory driver.
pub struct Flash<'d, MODE = Async> {
    pub(crate) inner: PeripheralRef<'d, FLASH>,
    pub(crate) _mode: PhantomData<MODE>,
}

impl<'d> Flash<'d, Blocking> {
    /// Create a new flash driver, usable in blocking mode.
    pub fn new_blocking(p: impl Peripheral<P = FLASH> + 'd) -> Self {
        into_ref!(p);

        Self {
            inner: p,
            _mode: PhantomData,
        }
    }
}

impl<'d, MODE> Flash<'d, MODE> {
    /// Blocking read.
    ///
    /// NOTE: `offset` is an offset from the flash start, NOT an absolute address.
    /// For example, to read address `0x0800_1234` you have to use offset `0x1234`.
    pub fn blocking_read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        if offset as usize + bytes.len() > FLASH_SIZE {
            return Err(Error::Size);
        }

        let start_address = FLASH_BASE + offset;
        let flash_data = unsafe { core::slice::from_raw_parts(start_address as *const u8, bytes.len()) };
        bytes.copy_from_slice(flash_data);
        Ok(())
    }

    /// Blocking write.
    ///
    /// NOTE: `offset` is an offset from the flash start, NOT an absolute address.
    /// For example, to write address `0x0800_1234` you have to use offset `0x1234`.
    pub fn blocking_write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        if offset as usize + bytes.len() > FLASH_SIZE {
            return Err(Error::Size);
        }

        if offset % WRITE_SIZE as u32 != 0 || bytes.len() % WRITE_SIZE != 0 {
            return Err(Error::Unaligned);
        }
    
        let mut address = FLASH_BASE + offset;
        trace!("Writing {} bytes at 0x{:x}", bytes.len(), address);
        for chunk in bytes.chunks(WRITE_SIZE) {
            unsafe { write_chunk_with_critical_section(address, chunk) }?;
            address += WRITE_SIZE as u32;
        }
        Ok(())
    }

    /// Blocking erase.
    ///
    /// NOTE: `from` and `to` are offsets from the flash start, NOT an absolute address.
    /// For example, to erase address `0x0801_0000` you have to use offset `0x1_0000`.
    pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        let start_address = FLASH_BASE + from;
        let end_address = FLASH_BASE + to;

        let sector_ret = ensure_sector_aligned(start_address, end_address);
        let page_ret = ensure_page_aligned(start_address, end_address);
        let use_sector = match (sector_ret, page_ret) {
            (Err(_), Err(_)) => return Err(Error::Unaligned),
            (Ok(_), _) => true,
            (Err(_), Ok(_)) => false,
        };

        trace!("Erasing from 0x{:x} to 0x{:x}, use_sector: {}", start_address, end_address, use_sector);

        let mut address = start_address;
        while address < end_address {
            if use_sector {
                let sector = get_sector(address);
                trace!("Erasing sector: {:?}", sector);
                unsafe { erase_unit_unlocked(&FlashUnit::Sector(sector)) }?;
                address += SECTOR_SIZE;
            } else {
                let page = get_page(address);
                trace!("Erasing page: {:?}", page);
                unsafe { erase_unit_unlocked(&FlashUnit::Page(page)) }?;
                address += PAGE_SIZE;
            }
        }
        Ok(())
    }
}

#[cfg(py32f072)]
pub mod values {
    pub const WRITE_SIZE: usize = 0x100;
    pub const READ_SIZE: usize = 1;
    pub const FLASH_SIZE: usize = 0x20000;

    pub const PAGE_SIZE: u32 = 0x100; // 256 Bytes
    pub const SECTOR_SIZE: u32 = 0x2000; // 8K Bytes
    pub const FLASH_BASE: u32 = 0x0800_0000;
    

    pub const FLASH_TIMING_PARAM: [u32; 8] = [0x1FFF011C, 0x1FFF011C, 0x1FFF011C, 0x1FFF011C, 0x1FFF011C, 0x1FFF0130, 0x1FFF011C, 0x1FFF011C];
}
use values::*;

pub(super) unsafe fn write_chunk_unlocked(address: u32, chunk: &[u8]) -> Result<(), Error> {
    low_level::clear_all_err();
    fence(Ordering::SeqCst);
    low_level::unlock();
    fence(Ordering::SeqCst);
    low_level::enable_blocking_write();
    fence(Ordering::SeqCst);

    let _on_drop = OnDrop::new(|| {
        low_level::disable_blocking_write();
        fence(Ordering::SeqCst);
        low_level::lock();
    });

    low_level::blocking_write(address, unwrap!(chunk.try_into()))
}

pub(super) unsafe fn write_chunk_with_critical_section(address: u32, chunk: &[u8]) -> Result<(), Error> {
    critical_section::with(|_| write_chunk_unlocked(address, chunk))
}

pub(super) unsafe fn erase_unit_unlocked(unit: &FlashUnit) -> Result<(), Error> {
    low_level::clear_all_err();
    fence(Ordering::SeqCst);
    low_level::unlock();
    fence(Ordering::SeqCst);

    let _on_drop = OnDrop::new(|| low_level::lock());

    low_level::blocking_erase_unit(unit)
}

pub(super) unsafe fn erase_unit_with_critical_section(unit: &FlashUnit) -> Result<(), Error> {
    critical_section::with(|_| erase_unit_unlocked(unit))
}

pub(super) fn get_sector(address: u32) -> FlashSector {
    let index = (address - FLASH_BASE) / SECTOR_SIZE;
    FlashSector {
        start: FLASH_BASE + index * SECTOR_SIZE,
    }
}

pub(super) fn get_page(address: u32) -> FlashPage {
    let index = (address - FLASH_BASE) / PAGE_SIZE;
    FlashPage {
        start: FLASH_BASE + index * PAGE_SIZE,
    }
}

pub(super) fn ensure_sector_aligned(
    start_address: u32,
    end_address: u32,
) -> Result<(), Error> {
    let mut address = start_address;
    while address < end_address {
        let sector = get_sector(address);
        if sector.start != address {
            return Err(Error::Unaligned);
        }
        address += SECTOR_SIZE;
    }
    if address != end_address {
        return Err(Error::Unaligned);
    }
    Ok(())
}

pub(super) fn ensure_page_aligned(
    start_address: u32,
    end_address: u32,
) -> Result<(), Error> {
    let mut address = start_address;
    while address < end_address {
        let page = get_page(address);
        if page.start != address {
            return Err(Error::Unaligned);
        }
        address += PAGE_SIZE;
    }
    if address != end_address {
        return Err(Error::Unaligned);
    }
    Ok(())
}

impl<MODE> embedded_storage::nor_flash::ErrorType for Flash<'_, MODE> {
    type Error = Error;
}

impl<MODE> embedded_storage::nor_flash::ReadNorFlash for Flash<'_, MODE> {
    const READ_SIZE: usize = READ_SIZE;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(offset, bytes)
    }

    fn capacity(&self) -> usize {
        FLASH_SIZE
    }
}

impl<MODE> embedded_storage::nor_flash::NorFlash for Flash<'_, MODE> {
    const WRITE_SIZE: usize = WRITE_SIZE;
    const ERASE_SIZE: usize = PAGE_SIZE as usize;

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(offset, bytes)
    }

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.blocking_erase(from, to)
    }
}

impl NorFlashError for Error {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::Size => NorFlashErrorKind::OutOfBounds,
            Self::Unaligned => NorFlashErrorKind::NotAligned,
            _ => NorFlashErrorKind::Other,
        }
    }
}