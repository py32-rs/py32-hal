// Adapted from embassy-stm32.
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32

use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicUsize, Ordering, compiler_fence, fence};
use core::task::{Context, Poll};

use embassy_sync::waitqueue::AtomicWaker;
use py32_metapac::dma::vals;

use super::word::{Word, WordSize};
use super::{Channel, Dir, Increment, Request, STATE, info};
use crate::_generated::DmaChannel;

mod ringbuffered;
use crate::interrupt::typelevel::Interrupt;
use crate::{interrupt, pac};
pub use ringbuffered::*;

pub(crate) struct ChannelInfo {
    pub(crate) dma: DmaInfo,
    pub(crate) num: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum DmaInfo {
    Dma(pac::dma::Dma),
}

/// DMA transfer options.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub struct TransferOptions {
    // /// Peripheral burst transfer configuration
    // pub pburst: Burst,
    // /// Memory burst transfer configuration
    // pub mburst: Burst,
    // /// Flow control configuration
    // pub flow_ctrl: FlowControl,
    // /// FIFO threshold for DMA FIFO mode. If none, direct mode is used.
    // pub fifo_threshold: Option<FifoThreshold>,
    /// Request priority level
    pub priority: Priority,
    /// Enable circular DMA
    ///
    /// Note:
    /// If you enable circular mode manually, you may want to build and `.await` the `Transfer` in a separate task.
    /// Since DMA in circular mode need manually stop, `.await` in current task would block the task forever.
    pub circular: bool,
    /// Enable half transfer interrupt
    pub half_transfer_ir: bool,
    /// Enable transfer complete interrupt
    pub complete_transfer_ir: bool,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            // pburst: Burst::Single,
            // mburst: Burst::Single,
            // flow_ctrl: FlowControl::Dma,
            // fifo_threshold: None,
            priority: Priority::VeryHigh,
            circular: false,
            half_transfer_ir: false,
            complete_transfer_ir: true,
        }
    }
}

/// DMA request priority
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Priority {
    /// Low Priority
    Low,
    /// Medium Priority
    Medium,
    /// High Priority
    High,
    /// Very High Priority
    VeryHigh,
}

impl From<Priority> for vals::Pl {
    fn from(value: Priority) -> Self {
        match value {
            Priority::Low => pac::dma::vals::Pl::LOW,
            Priority::Medium => pac::dma::vals::Pl::MEDIUM,
            Priority::High => pac::dma::vals::Pl::HIGH,
            Priority::VeryHigh => pac::dma::vals::Pl::VERYHIGH,
        }
    }
}

impl From<WordSize> for vals::Size {
    fn from(raw: WordSize) -> Self {
        match raw {
            WordSize::OneByte => Self::BITS8,
            WordSize::TwoBytes => Self::BITS16,
            WordSize::FourBytes => Self::BITS32,
        }
    }
}

impl From<Dir> for vals::Dir {
    fn from(raw: Dir) -> Self {
        match raw {
            Dir::MemoryToPeripheral => Self::MEMORYTOPERIPHERAL,
            Dir::PeripheralToMemory | Dir::MemoryToMemory => Self::PERIPHERALTOMEMORY,
        }
    }
}

pub(crate) struct ChannelState {
    waker: AtomicWaker,
    complete_count: AtomicUsize,
}

impl ChannelState {
    pub(crate) const NEW: Self = Self {
        waker: AtomicWaker::new(),
        complete_count: AtomicUsize::new(0),
    };
}

pub(crate) unsafe fn init(
    cs: critical_section::CriticalSection,
    dma_priority: interrupt::Priority,
) {
    unsafe {
        foreach_interrupt! {
            ($peri:ident, dma, $block:ident, $signal_name:ident, $irq:ident) => {
                crate::interrupt::typelevel::$irq::set_priority_with_cs(cs, dma_priority);
                crate::interrupt::typelevel::$irq::enable();
            };
        }
        crate::_generated::init_dma();
    }
}

pub(crate) unsafe fn on_irq(channel: DmaChannel) {
    let info = info(channel);
    let state = &STATE[channel as usize];
    match info.dma {
        DmaInfo::Dma(r) => {
            let isr = r.isr().read();
            let cr = r.st(info.num).cr();

            if isr.teif(info.num) {
                panic!(
                    "DMA: error on DMA@{:08x} channel {}",
                    r.as_ptr() as u32,
                    info.num
                );
            }

            let mut activity = false;
            if isr.htif(info.num) && cr.read().htie() {
                // Acknowledge half transfer complete interrupt
                r.ifcr().write(|w| w.set_htif(info.num, true));
                activity = true;
            }
            if isr.tcif(info.num) && cr.read().tcie() {
                // Acknowledge transfer complete interrupt
                r.ifcr().write(|w| w.set_tcif(info.num, true));
                #[cfg(not(armv6m))]
                state.complete_count.fetch_add(1, Ordering::Release);
                #[cfg(armv6m)]
                critical_section::with(|_| {
                    let x = state.complete_count.load(Ordering::Relaxed);
                    state.complete_count.store(x + 1, Ordering::Release);
                });
                // PY32 retains EN at the end of a non-circular transfer.
                if !cr.read().circ() && r.st(info.num).ndtr().read() == 0 {
                    cr.modify(|w| w.set_en(false));
                }
                activity = true;
            }
            if !activity {
                return;
            }

            state.waker.wake();
        }
    }
}
impl<'d> Channel<'d> {
    fn info(&self) -> &'static super::ChannelInfo {
        super::info(self.channel)
    }

    unsafe fn configure(
        &self,
        _request: Request,
        dir: Dir,
        peri_addr: *const u32,
        mem_addr: *mut u32,
        mem_len: usize,
        incr_mem: Increment,
        mem_size: WordSize,
        peri_size: WordSize,
        options: TransferOptions,
    ) {
        // "Preceding reads and writes cannot be moved past subsequent writes."
        fence(Ordering::SeqCst);

        let info = self.info();

        match info.num / 4 {
            0 => {
                pac::SYSCFG.cfgr3().modify(|w| {
                    w.set_dma_map(info.num % 4, _request);
                });
            }
            #[cfg(any(py32f040, py32f071, py32f072, py32m070))]
            1 => {
                pac::SYSCFG.cfgr4().modify(|w| {
                    w.set_dma_map(info.num % 4, _request);
                });
            }
            _ => panic!("Invalid DMA channel number"),
        }

        match self.info().dma {
            DmaInfo::Dma(r) => {
                assert!(mem_len > 0 && mem_len <= 0xFFFF);

                let state: &ChannelState = &STATE[self.channel as usize];
                let ch = r.st(info.num);

                state.complete_count.store(0, Ordering::Release);
                self.clear_irqs();

                if ch.cr().read().en() {
                    warn!(
                        "DMA channel is still enabled after previous transfer. Make sure that transfer was dropped."
                    );
                    ch.cr().modify(|w| w.set_en(false));
                    while ch.cr().read().en() {}
                }

                ch.par().write_value(peri_addr as u32);
                ch.mar().write_value(mem_addr as u32);
                ch.ndtr().write_value(mem_len as u32);
                ch.cr().write(|w| {
                    w.set_psize(peri_size.into());
                    w.set_msize(mem_size.into());
                    match incr_mem {
                        Increment::None => {
                            w.set_minc(false);
                            w.set_pinc(false);
                        }
                        Increment::Peripheral => {
                            w.set_minc(false);
                            w.set_pinc(true);
                        }
                        Increment::Memory => {
                            w.set_minc(true);
                            w.set_pinc(false);
                        }
                        Increment::Both => {
                            w.set_minc(true);
                            w.set_pinc(true);
                        }
                    }
                    w.set_mem2mem(dir == Dir::MemoryToMemory);
                    w.set_dir(dir.into());
                    w.set_teie(true);
                    w.set_tcie(options.complete_transfer_ir);
                    w.set_htie(options.half_transfer_ir);
                    w.set_circ(options.circular);
                    w.set_pl(options.priority.into());
                    w.set_en(false); // don't start yet
                });
            }
        }
    }

    /// Starts the channel by enabling it.
    ///
    /// This function also unsuspends (resumes) the channel.
    fn start(&self) {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(r) => {
                let ch = r.st(info.num);
                ch.cr().modify(|w| w.set_en(true));
            }
        }
    }

    fn clear_irqs(&self) {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(r) => {
                r.ifcr().write(|w| {
                    w.set_htif(info.num, true);
                    w.set_tcif(info.num, true);
                    w.set_teif(info.num, true);
                });
            }
        }
    }

    fn request_pause(&self) {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(r) => {
                // Disable the channel without overwriting the existing configuration
                r.st(info.num).cr().modify(|w| {
                    w.set_en(false);
                });
            }
        }
    }

    fn request_resume(&self) {
        self.start()
    }

    /// Resets the channel. The configuration is not preserved.
    fn request_reset(&self) {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(r) => {
                // Disable the channel. Keep the IEs enabled so the irqs still fire.
                r.st(info.num).cr().write(|w| {
                    w.set_teie(true);
                    w.set_tcie(true);
                });
            }
        }

        while self.is_running() {}
    }

    fn clear_flags_and_unpend(&self) {
        self.clear_irqs();
    }

    fn pause_and_wait(&self) {
        self.request_pause();
        while self.is_running() {}
        self.clear_flags_and_unpend();
    }

    fn is_running(&self) -> bool {
        let info = self.info();
        match info.dma {
            DmaInfo::Dma(r) => r.st(info.num).cr().read().en(),
        }
    }

    fn get_remaining_transfers(&self) -> u32 {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(r) => r.st(info.num).ndtr().read() as u32,
        }
    }

    fn disable_circular_mode(&self) {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(regs) => regs.st(info.num).cr().modify(|w| {
                w.set_circ(false);
            }),
        }
    }

    fn enable_circular_mode(&self) {
        let info = self.info();
        match self.info().dma {
            DmaInfo::Dma(regs) => regs.st(info.num).cr().modify(|w| {
                w.set_circ(true);
            }),
        }
    }

    fn poll_stop(&self) -> Poll<()> {
        compiler_fence(Ordering::SeqCst);

        if !self.is_running() {
            fence(Ordering::Acquire);

            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    /// Create a memory DMA transfer (memory to memory), using raw pointers.
    pub unsafe fn transfer<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        buf: *const [PW],
        dest_addr: *mut MW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        self.transfer_raw(request, buf as *const PW, buf.len(), dest_addr, options)
    }

    /// Create a memory DMA transfer (memory to memory), using raw pointers.
    pub unsafe fn transfer_raw<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        src_addr: *const MW,
        src_size: usize,
        dest_addr: *mut PW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        self.configure(
            request,
            Dir::MemoryToMemory,
            src_addr as *mut u32,
            dest_addr as *mut u32,
            src_size,
            Increment::Both,
            // PAR is the source and MAR is the destination in memory-to-memory mode.
            PW::size(),
            MW::size(),
            options,
        );
        self.start();
        Transfer {
            channel: self.reborrow(),
        }
    }

    /// Create a read DMA transfer (peripheral to memory).
    pub unsafe fn read<'a, W: Word>(
        &'a mut self,
        request: Request,
        peri_addr: *mut W,
        buf: &'a mut [W],
        options: TransferOptions,
    ) -> Transfer<'a> {
        self.read_raw(request, peri_addr, buf, options)
    }

    /// Create a read DMA transfer (peripheral to memory), using raw pointers.
    pub unsafe fn read_raw<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        peri_addr: *mut PW,
        buf: *mut [MW],
        options: TransferOptions,
    ) -> Transfer<'a> {
        let mem_len = buf.len();

        self.configure(
            request,
            Dir::PeripheralToMemory,
            peri_addr as *const u32,
            buf as *mut MW as *mut u32,
            mem_len,
            Increment::Memory,
            MW::size(),
            PW::size(),
            options,
        );
        self.start();
        Transfer {
            channel: self.reborrow(),
        }
    }

    /// Create a read DMA transfer (peripheral to memory), writing the same value repeatedly.
    pub unsafe fn read_raw_repeated<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        repeated: *mut MW,
        count: usize,
        peri_addr: *mut PW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        assert!(count > 0 && count <= 0xFFFF);

        self.configure(
            request,
            Dir::PeripheralToMemory,
            peri_addr as *const u32,
            repeated as *const MW as *mut u32,
            count,
            Increment::None,
            MW::size(),
            PW::size(),
            options,
        );
        self.start();
        Transfer {
            channel: self.reborrow(),
        }
    }

    /// Create a write DMA transfer (memory to peripheral).
    pub unsafe fn write<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        buf: &'a [MW],
        peri_addr: *mut PW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        self.write_raw(request, buf, peri_addr, options)
    }

    /// Create a write DMA transfer (memory to peripheral), using raw pointers.
    pub unsafe fn write_raw<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        buf: *const [MW],
        peri_addr: *mut PW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        let mem_len = buf.len();

        self.configure(
            request,
            Dir::MemoryToPeripheral,
            peri_addr as *const u32,
            buf as *const MW as *mut u32,
            mem_len,
            Increment::Memory,
            MW::size(),
            PW::size(),
            options,
        );
        self.start();
        Transfer {
            channel: self.reborrow(),
        }
    }

    /// Create a write DMA transfer (memory to peripheral), writing the same value repeatedly.
    pub unsafe fn write_repeated<'a, W: Word>(
        &'a mut self,
        request: Request,
        repeated: &'a W,
        count: usize,
        peri_addr: *mut W,
        options: TransferOptions,
    ) -> Transfer<'a> {
        assert!(count > 0 && count <= 0xFFFF);

        self.configure(
            request,
            Dir::MemoryToPeripheral,
            peri_addr as *const u32,
            repeated as *const W as *mut u32,
            count,
            Increment::None,
            W::size(),
            W::size(),
            options,
        );
        self.start();
        Transfer {
            channel: self.reborrow(),
        }
    }

    /// Create a memory DMA transfer (memory to memory) to a fixed destination address.
    ///
    /// This transfers data from a memory buffer (`buf`) to a fixed destination address (`dest_addr`).
    ///
    /// This is specifically required for peripherals like the DFSDM, which need
    /// memory-to-memory DMA transfers to write data into their internal input registers
    /// (e.g., `DATINR`), rather than standard memory-to-peripheral transfers.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the destination address is valid and that the
    /// DMA transfer does not violate Rust's aliasing rules for the duration of the transfer.
    pub unsafe fn write_mem2mem<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        buf: &'a [MW],
        dest_addr: *mut PW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        self.write_mem2mem_raw(request, buf, dest_addr, options)
    }

    /// Create a memory DMA transfer (memory to memory) to a fixed destination address, using raw pointers.
    ///
    /// This is the raw pointer variant of [`write_mem2mem`](Self::write_mem2mem).
    /// It transfers data from a raw source slice (`buf`) to a fixed destination address (`dest_addr`).
    ///
    /// This is specifically used for peripherals like the DFSDM, which require
    /// memory-to-memory DMA to feed data into their internal registers.
    ///
    /// Note: The arguments are ordered logically as `(source, destination)` to avoid
    /// the internal `peri_addr`/`mem_addr` ambiguity present in the underlying
    /// `Dir::MemoryToMemory` hardware configuration.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the source and destination pointers are valid
    /// and properly aligned for the duration of the transfer.
    pub unsafe fn write_mem2mem_raw<'a, MW: Word, PW: Word>(
        &'a mut self,
        request: Request,
        buf: *const [MW],
        dest_addr: *mut PW,
        options: TransferOptions,
    ) -> Transfer<'a> {
        let mem_len = buf.len();

        self.configure(
            request,
            Dir::MemoryToMemory,
            buf as *const MW as *mut u32,
            dest_addr as *mut u32,
            mem_len,
            Increment::Peripheral,
            // PAR is the source and MAR is the destination in memory-to-memory mode.
            PW::size(),
            MW::size(),
            options,
        );
        self.start();
        Transfer {
            channel: self.reborrow(),
        }
    }
}

/// DMA transfer.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Transfer<'a> {
    channel: Channel<'a>,
}

impl<'a> Transfer<'a> {
    /// Request the transfer to pause, keeping the existing configuration for this channel.
    ///
    /// To resume the transfer, call [`request_resume`](Self::request_resume) again.
    /// This doesn't immediately stop the transfer, you have to wait until [`is_running`](Self::is_running) returns false.
    pub fn request_pause(&mut self) {
        self.channel.request_pause()
    }

    /// Request the transfer to resume after having been paused.
    pub fn request_resume(&mut self) {
        self.channel.request_resume()
    }

    /// Request the DMA to reset.
    ///
    /// The configuration for this channel will **not be preserved**. If you need to restart the transfer
    /// at a later point with the same configuration, see [`request_pause`](Self::request_pause) instead.
    pub fn request_reset(&mut self) {
        self.channel.request_reset()
    }

    /// Return whether this transfer is still running.
    ///
    /// If this returns `false`, it can be because either the transfer finished, or
    /// it was requested to stop early with [`request_pause`](Self::request_pause).
    pub fn is_running(&mut self) -> bool {
        self.channel.is_running()
    }

    /// Gets the total remaining transfers for the channel
    /// Note: this will be zero for transfers that completed without cancellation.
    pub fn get_remaining_transfers(&self) -> u32 {
        self.channel.get_remaining_transfers()
    }

    /// Blocking wait until the transfer finishes.
    pub fn blocking_wait(mut self) {
        while self.is_running() {}

        // "Subsequent reads and writes cannot be moved ahead of preceding reads."
        fence(Ordering::SeqCst);

        core::mem::forget(self);
    }
}

impl<'a> Drop for Transfer<'a> {
    fn drop(&mut self) {
        self.request_reset();
        while self.is_running() {}
        self.channel.clear_flags_and_unpend();

        // "Subsequent reads and writes cannot be moved ahead of preceding reads."
        fence(Ordering::SeqCst);
    }
}

impl<'a> Unpin for Transfer<'a> {}
impl<'a> Future for Transfer<'a> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state: &ChannelState = &STATE[self.channel.channel as usize];

        state.waker.register(cx.waker());

        compiler_fence(Ordering::SeqCst);
        if self.is_running() {
            Poll::Pending
        } else {
            fence(Ordering::Acquire);

            Poll::Ready(())
        }
    }
}
