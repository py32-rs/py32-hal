//! Serial Peripheral Interface (SPI)

// The following code is modified from embassy-stm32(Rust) and PY32F0XX_HAL_Driver(C)
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

#![macro_use]

use core::marker::PhantomData;

use embassy_hal_internal::{Peripheral, PeripheralRef};
#[cfg(feature = "time")]
use embassy_time::{Duration, Instant};

#[cfg(dma)]
use crate::dma::ChannelAndRequest;
use crate::gpio::{AfType, AnyPin, OutputType, Pull, SealedPin as _, Speed};
use crate::mode::{Async, Blocking, Mode};
#[allow(unused_imports)]
use crate::rcc::{RccInfo, SealedRccPeripheral};
use crate::time::Hertz;
use crate::{pac, peripherals};

// ====================
// SPI communication mode (Master / Slave)
// ====================

mod sealed {
    pub trait SealedCommunicationMode {}
}

/// SPI communication mode.
#[allow(private_bounds)]
pub trait CommunicationMode: sealed::SealedCommunicationMode {
    #[cfg(spi_v1)]
    const MSTR_VALUE: pac::spi::vals::Mstr;
    #[cfg(not(spi_v1))]
    const MSTR_VALUE: pac::spi::vals::Mstr;
    const SSI_VALUE: bool;
}

/// Master mode.
pub struct Master;
/// Slave mode.
pub struct Slave;

impl sealed::SealedCommunicationMode for Master {}
impl CommunicationMode for Master {
    #[cfg(spi_v1)]
    const MSTR_VALUE: pac::spi::vals::Mstr = pac::spi::vals::Mstr::MASTER;
    #[cfg(not(spi_v1))]
    const MSTR_VALUE: pac::spi::vals::Mstr = pac::spi::vals::Mstr::MASTER;
    const SSI_VALUE: bool = true;
}

impl sealed::SealedCommunicationMode for Slave {}
impl CommunicationMode for Slave {
    #[cfg(spi_v1)]
    const MSTR_VALUE: pac::spi::vals::Mstr = pac::spi::vals::Mstr::SLAVE;
    #[cfg(not(spi_v1))]
    const MSTR_VALUE: pac::spi::vals::Mstr = pac::spi::vals::Mstr::SLAVE;
    const SSI_VALUE: bool = false;
}

// ====================
// Bit order
// ====================

/// SPI bit order.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BitOrder {
    /// Most significant bit first.
    MsbFirst,
    /// Least significant bit first.
    LsbFirst,
}

/// SPI data width.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DataWidth {
    /// 8-bit data frame.
    Bits8,
    /// 16-bit data frame.
    Bits16,
}

// ====================
// NSS mode
// ====================

/// SPI NSS (chip select) management mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum NssMode {
    /// Software NSS (SSM=1, SSI controlled internally).
    Soft,
    /// Hardware NSS input (SSM=0, NSS pin is input).
    HardInput,
    /// Hardware NSS output (SSM=0, SSOE=1, NSS pin driven low during transfer).
    HardOutput,
}

// ====================
// Error
// ====================

/// SPI error.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Overrun error.
    Overrun,
    /// Mode fault error.
    ModeFault,
    /// Timeout.
    Timeout,
    /// CRC error (SPI v2 only).
    Crc,
}

// ====================
// Config
// ====================

/// SPI configuration.
#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct Config {
    /// SPI clock frequency.
    pub frequency: Hertz,
    /// SPI mode (polarity + phase).
    pub mode: embedded_hal_1::spi::Mode,
    /// Bit order.
    pub bit_order: BitOrder,
    /// Data width (8-bit or 16-bit).
    pub data_width: DataWidth,
    /// NSS management mode.
    pub nss: NssMode,
    /// Enable slave fast mode (SPI v1 only, ignored on v2).
    pub slave_fast_mode: bool,
    /// Timeout.
    #[cfg(feature = "time")]
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            frequency: Hertz(1_000_000),
            mode: embedded_hal_1::spi::MODE_0,
            bit_order: BitOrder::MsbFirst,
            data_width: DataWidth::Bits8,
            nss: NssMode::Soft,
            slave_fast_mode: false,
            #[cfg(feature = "time")]
            timeout: Duration::from_millis(1000),
        }
    }
}

impl Config {
    fn sck_af(&self) -> AfType {
        AfType::output(OutputType::PushPull, Speed::VeryHigh)
    }

    fn mosi_af(&self) -> AfType {
        AfType::output(OutputType::PushPull, Speed::VeryHigh)
    }

    fn miso_af(&self) -> AfType {
        AfType::input(Pull::None)
    }

    fn cs_af(&self) -> AfType {
        AfType::output(OutputType::PushPull, Speed::VeryHigh)
    }
}

// ====================
// Spi driver struct
// ====================

/// SPI driver.
pub struct Spi<'d, M: Mode, CM: CommunicationMode> {
    info: &'static Info,
    #[allow(dead_code)]
    kernel_clock: Hertz,
    sck: Option<PeripheralRef<'d, AnyPin>>,
    mosi: Option<PeripheralRef<'d, AnyPin>>,
    miso: Option<PeripheralRef<'d, AnyPin>>,
    cs: Option<PeripheralRef<'d, AnyPin>>,
    #[cfg(dma)]
    tx_dma: Option<ChannelAndRequest<'d>>,
    #[cfg(dma)]
    rx_dma: Option<ChannelAndRequest<'d>>,
    #[cfg(feature = "time")]
    timeout: Duration,
    data_width: DataWidth,
    _phantom: PhantomData<(M, CM)>,
}

// ====================
// Baud rate computation
// ====================

fn compute_baud_rate(kernel_clock: Hertz, frequency: Hertz) -> pac::spi::vals::Br {
    let kernel = kernel_clock.0;
    let freq = frequency.0;
    if freq >= kernel / 2 {
        return pac::spi::vals::Br::DIV2;
    }
    if freq >= kernel / 4 {
        return pac::spi::vals::Br::DIV4;
    }
    if freq >= kernel / 8 {
        return pac::spi::vals::Br::DIV8;
    }
    if freq >= kernel / 16 {
        return pac::spi::vals::Br::DIV16;
    }
    if freq >= kernel / 32 {
        return pac::spi::vals::Br::DIV32;
    }
    if freq >= kernel / 64 {
        return pac::spi::vals::Br::DIV64;
    }
    if freq >= kernel / 128 {
        return pac::spi::vals::Br::DIV128;
    }
    pac::spi::vals::Br::DIV256
}

// ====================
// Core transfer helpers
// ====================

fn wait_txe(regs: pac::spi::Spi, timeout: &mut TimeoutCtx) -> Result<(), Error> {
    while !regs.sr().read().txe() {
        timeout.check()?;
        let sr = regs.sr().read();
        if sr.ovr() {
            let _ = regs.dr().read();
            let _ = regs.sr().read();
            return Err(Error::Overrun);
        }
        if sr.modf() {
            regs.cr1().modify(|w| w.set_spe(false));
            regs.cr1().modify(|w| w.set_spe(true));
            return Err(Error::ModeFault);
        }
    }
    Ok(())
}

fn wait_rxne(regs: pac::spi::Spi, timeout: &mut TimeoutCtx) -> Result<(), Error> {
    while !regs.sr().read().rxne() {
        timeout.check()?;
        let sr = regs.sr().read();
        if sr.ovr() {
            let _ = regs.dr().read();
            let _ = regs.sr().read();
            return Err(Error::Overrun);
        }
        if sr.modf() {
            regs.cr1().modify(|w| w.set_spe(false));
            regs.cr1().modify(|w| w.set_spe(true));
            return Err(Error::ModeFault);
        }
    }
    Ok(())
}

fn wait_bsy(regs: pac::spi::Spi, timeout: &mut TimeoutCtx) -> Result<(), Error> {
    while regs.sr().read().bsy() {
        timeout.check()?;
    }
    Ok(())
}

/// Core full-duplex transfer: write `tx` and receive into `rx`.
fn transfer_u8(
    regs: pac::spi::Spi,
    tx: &[u8],
    rx: &mut [u8],
    timeout: &mut TimeoutCtx,
) -> Result<(), Error> {
    let max_len = core::cmp::max(tx.len(), rx.len());
    for i in 0..max_len {
        let wb = if i < tx.len() { tx[i] } else { 0xFF };
        wait_txe(regs, timeout)?;
        regs.dr().write(|w| w.set_dr(wb as u16));
        wait_rxne(regs, timeout)?;
        let rb = regs.dr().read().dr() as u8;
        if i < rx.len() {
            rx[i] = rb;
        }
    }
    wait_bsy(regs, timeout)
}

/// Core in-place transfer: write and read the same buffer.
fn transfer_in_place_u8(
    regs: pac::spi::Spi,
    buf: &mut [u8],
    timeout: &mut TimeoutCtx,
) -> Result<(), Error> {
    for b in buf.iter_mut() {
        let wb = *b;
        wait_txe(regs, timeout)?;
        regs.dr().write(|w| w.set_dr(wb as u16));
        wait_rxne(regs, timeout)?;
        *b = regs.dr().read().dr() as u8;
    }
    wait_bsy(regs, timeout)
}

/// Core full-duplex transfer: write `tx` and receive into `rx` (16-bit).
fn transfer_u16(
    regs: pac::spi::Spi,
    tx: &[u16],
    rx: &mut [u16],
    timeout: &mut TimeoutCtx,
) -> Result<(), Error> {
    let max_len = core::cmp::max(tx.len(), rx.len());
    for i in 0..max_len {
        let wb = if i < tx.len() { tx[i] } else { 0xFFFF };
        wait_txe(regs, timeout)?;
        regs.dr().write(|w| w.set_dr(wb));
        wait_rxne(regs, timeout)?;
        let rb = regs.dr().read().dr();
        if i < rx.len() {
            rx[i] = rb;
        }
    }
    wait_bsy(regs, timeout)
}

/// Core in-place transfer: write and read the same buffer (16-bit).
fn transfer_in_place_u16(
    regs: pac::spi::Spi,
    buf: &mut [u16],
    timeout: &mut TimeoutCtx,
) -> Result<(), Error> {
    for b in buf.iter_mut() {
        let wb = *b;
        wait_txe(regs, timeout)?;
        regs.dr().write(|w| w.set_dr(wb));
        wait_rxne(regs, timeout)?;
        *b = regs.dr().read().dr();
    }
    wait_bsy(regs, timeout)
}

// ====================
// Timeout helper
// ====================

struct TimeoutCtx {
    #[cfg(feature = "time")]
    deadline: Instant,
}

#[cfg(feature = "time")]
impl TimeoutCtx {
    fn from_duration(d: Duration) -> Self {
        Self {
            deadline: Instant::now() + d,
        }
    }

    fn check(&mut self) -> Result<(), Error> {
        if Instant::now() > self.deadline {
            return Err(Error::Timeout);
        }
        Ok(())
    }
}

#[cfg(not(feature = "time"))]
impl TimeoutCtx {
    fn from_duration(_d: ()) -> Self {
        Self {}
    }

    fn check(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

// ====================
// Info, State, Instance
// ====================

struct Info {
    regs: pac::spi::Spi,
    rcc: RccInfo,
}

struct State {}

impl State {
    const fn new() -> Self {
        Self {}
    }
}

peri_trait!();

pin_trait!(SckPin, Instance);
pin_trait!(MosiPin, Instance);
pin_trait!(MisoPin, Instance);
pin_trait!(CsPin, Instance);
#[cfg(dma)] dma_trait!(RxDma, Instance);
#[cfg(dma)] dma_trait!(TxDma, Instance);

foreach_peripheral!(
    (spi, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info{
                    regs: crate::pac::$inst,
                    rcc: crate::peripherals::$inst::RCC_INFO,
                };
                &INFO
            }
            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for peripherals::$inst {}
    };
);

// ====================
// Common impl (init, new_inner, drop)
// ====================

impl<'d, M: Mode, CM: CommunicationMode> Spi<'d, M, CM> {
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        sck: Option<PeripheralRef<'d, AnyPin>>,
        mosi: Option<PeripheralRef<'d, AnyPin>>,
        miso: Option<PeripheralRef<'d, AnyPin>>,
        cs: Option<PeripheralRef<'d, AnyPin>>,
        #[cfg(dma)] tx_dma: Option<ChannelAndRequest<'d>>,
        #[cfg(dma)] rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Self {
        let mut this = Self {
            info: T::info(),
            kernel_clock: T::frequency(),
            sck,
            mosi,
            miso,
            cs,
            #[cfg(dma)] tx_dma,
            #[cfg(dma)] rx_dma,
            #[cfg(feature = "time")]
            timeout: config.timeout,
            data_width: config.data_width,
            _phantom: PhantomData,
        };
        this.enable_and_init(config);
        this
    }

    fn enable_and_init(&mut self, config: Config) {
        let br = compute_baud_rate(self.kernel_clock, config.frequency);
        let cpha = match config.mode.phase {
            embedded_hal_1::spi::Phase::CaptureOnFirstTransition => pac::spi::vals::Cpha::FIRSTEDGE,
            embedded_hal_1::spi::Phase::CaptureOnSecondTransition => pac::spi::vals::Cpha::SECONDEDGE,
        };
        let cpol = match config.mode.polarity {
            embedded_hal_1::spi::Polarity::IdleLow => pac::spi::vals::Cpol::IDLELOW,
            embedded_hal_1::spi::Polarity::IdleHigh => pac::spi::vals::Cpol::IDLEHIGH,
        };
        let lsbfirst = match config.bit_order {
            BitOrder::MsbFirst => pac::spi::vals::Lsbfirst::MSBFIRST,
            BitOrder::LsbFirst => pac::spi::vals::Lsbfirst::LSBFIRST,
        };
        let (ssm, ssoe) = match config.nss {
            NssMode::Soft => (true, false),
            NssMode::HardInput => (false, false),
            NssMode::HardOutput => (false, true),
        };

        self.info.rcc.enable_and_reset();

        let regs = self.info.regs;

        // Configure CR1
        regs.cr1().modify(|w| {
            w.set_cpha(cpha);
            w.set_cpol(cpol);
            w.set_mstr(CM::MSTR_VALUE);
            w.set_br(br);
            w.set_lsbfirst(lsbfirst);
            w.set_ssi(CM::SSI_VALUE);
            w.set_ssm(ssm);
            // Always use full-duplex (FULLDUPLEX), even in "RX-only" mode (no MOSI pin).
            // Per reference manual §27.3.8: setting RXONLY=1 causes SCK to run
            // continuously — we cannot stop it without disabling SPE or clearing RXONLY.
            // Instead, we do "fake RX-only": write a dummy byte to TXDR for each byte
            // we want to receive, generating one SCK pulse per byte.
            // Setting OUTPUTDISABLED here would hang the transfer.
            w.set_rxonly(pac::spi::vals::Rxonly::FULLDUPLEX);
            w.set_bidimode(pac::spi::vals::Bidimode::UNIDIRECTIONAL);
            w.set_bidioe(pac::spi::vals::Bidioe::RECEIVE);

            // SPI v2: data size in CR1.DDF
            #[cfg(not(spi_v1))]
            w.set_ddf(false); // 8-bit by default

            // SPI v2: CRC disable
            #[cfg(not(spi_v1))]
            w.set_crcen(false);

            w.set_spe(true);
        });

        // Configure CR2
        regs.cr2().modify(|w| {
            w.set_ssoe(ssoe);

            // FRXTH and DS depend on data width
            match config.data_width {
                DataWidth::Bits8 => {
                    w.set_frxth(pac::spi::vals::Frxth::QUARTER);
                    // SPI v1: data size in CR2.DS = 0 for 8-bit
                    #[cfg(spi_v1)]
                    w.set_ds(false);
                }
                DataWidth::Bits16 => {
                    w.set_frxth(pac::spi::vals::Frxth::HALF);
                    // SPI v1: data size in CR2.DS = 1 for 16-bit
                    #[cfg(spi_v1)]
                    w.set_ds(true);
                }
            }

            #[cfg(spi_v1)]
            w.set_slvfm(config.slave_fast_mode);
        });
    }

    fn timeout_ctx(&self) -> TimeoutCtx {
        TimeoutCtx::from_duration({
            #[cfg(feature = "time")]
            {
                self.timeout
            }
            #[cfg(not(feature = "time"))]
            {
                ()
            }
        })
    }

    /// Set the data width at runtime.
    ///
    /// Must be called while SPI is disabled (SPE=0), or you must disable SPI first.
    /// Per reference manual §27.3.7, CR2.DS and CR2.FRXTH can only be changed when SPE=0.
    pub fn set_data_width(&mut self, width: DataWidth) {
        let regs = self.info.regs;
        let was_enabled = regs.cr1().read().spe();
        if was_enabled {
            regs.cr1().modify(|w| w.set_spe(false));
        }
        regs.cr2().modify(|w| {
            match width {
                DataWidth::Bits8 => {
                    w.set_frxth(pac::spi::vals::Frxth::QUARTER);
                    #[cfg(spi_v1)]
                    w.set_ds(false);
                }
                DataWidth::Bits16 => {
                    w.set_frxth(pac::spi::vals::Frxth::HALF);
                    #[cfg(spi_v1)]
                    w.set_ds(true);
                }
            }
        });
        if was_enabled {
            regs.cr1().modify(|w| w.set_spe(true));
        }
        self.data_width = width;
    }
}

impl<'d, M: Mode, CM: CommunicationMode> Drop for Spi<'d, M, CM> {
    fn drop(&mut self) {
        let regs = self.info.regs;
        regs.cr1().modify(|w| w.set_spe(false));
        self.sck.as_ref().map(|x| x.set_as_disconnected());
        self.mosi.as_ref().map(|x| x.set_as_disconnected());
        self.miso.as_ref().map(|x| x.set_as_disconnected());
        self.cs.as_ref().map(|x| x.set_as_disconnected());
        self.info.rcc.disable();
    }
}

// ====================
// Blocking Master constructors
// ====================

impl<'d> Spi<'d, Blocking, Master> {
    /// Create a new blocking SPI master driver.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, config.mosi_af()),
            new_pin!(miso, config.miso_af()),
            None,
            #[cfg(dma)] None,
            #[cfg(dma)] None,
            config,
        )
    }

    /// Create a new blocking SPI master driver with CS pin.
    pub fn new_blocking_with_cs<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        cs: impl Peripheral<P = impl CsPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, config.mosi_af()),
            new_pin!(miso, config.miso_af()),
            new_pin!(cs, config.cs_af()),
            #[cfg(dma)] None,
            #[cfg(dma)] None,
            config,
        )
    }

    /// Create a new blocking SPI master driver in TX-only mode (no MISO pin).
    pub fn new_blocking_txonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, config.mosi_af()),
            None,
            None,
            #[cfg(dma)] None,
            #[cfg(dma)] None,
            config,
        )
    }

    /// Create a new blocking SPI master driver in RX-only mode (no MOSI pin).
    pub fn new_blocking_rxonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            None,
            new_pin!(miso, config.miso_af()),
            None,
            #[cfg(dma)] None,
            #[cfg(dma)] None,
            config,
        )
    }
}

// ====================
// Blocking Slave constructors
// ====================

impl<'d> Spi<'d, Blocking, Slave> {
    /// Create a new blocking SPI slave driver.
    pub fn new_blocking_slave<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        cs: impl Peripheral<P = impl CsPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, config.mosi_af()),
            new_pin!(miso, config.miso_af()),
            new_pin!(cs, AfType::input(Pull::None)),
            #[cfg(dma)] None,
            #[cfg(dma)] None,
            config,
        )
    }
}

// ====================
// Async Master constructors (with DMA)
// ====================

#[cfg(dma)]
impl<'d> Spi<'d, Async, Master> {
    /// Create a new async SPI master driver with DMA.
    pub fn new<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        rx_dma: impl Peripheral<P = impl RxDma<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, config.mosi_af()),
            new_pin!(miso, config.miso_af()),
            None,
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        )
    }

    /// Create a new async SPI master driver with CS pin and DMA.
    pub fn new_with_cs<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        cs: impl Peripheral<P = impl CsPin<T>> + 'd,
        tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        rx_dma: impl Peripheral<P = impl RxDma<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, config.mosi_af()),
            new_pin!(miso, config.miso_af()),
            new_pin!(cs, config.cs_af()),
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        )
    }
}

// ====================
// Blocking transfer methods (available for any M: Mode)
// Implemented on Master
// ====================

impl<'d, M: Mode> Spi<'d, M, Master> {
    /// Blocking write (Master).
    pub fn blocking_write(&mut self, words: &[u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();

        if self.miso.is_some() {
            // Full-duplex: write and discard received data
            for &w in words {
                wait_txe(regs, &mut timeout)?;
                regs.dr().write(|d| d.set_dr(w as u16));
                wait_rxne(regs, &mut timeout)?;
                let _ = regs.dr().read();
            }
            wait_bsy(regs, &mut timeout)?;
            // Clear overrun flag (2Lines TX: RX data builds up)
            let _ = regs.dr().read();
            let _ = regs.sr().read();
            Ok(())
        } else {
            // TX-only: no MISO, just write
            for &w in words {
                wait_txe(regs, &mut timeout)?;
                regs.dr().write(|d| d.set_dr(w as u16));
            }
            wait_bsy(regs, &mut timeout)?;
            Ok(())
        }
    }

    /// Blocking read (Master). Sends 0xFF dummy bytes to generate clock.
    pub fn blocking_read(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        for b in words.iter_mut() {
            wait_txe(regs, &mut timeout)?;
            regs.dr().write(|d| d.set_dr(0x00FFu16));
            wait_rxne(regs, &mut timeout)?;
            *b = regs.dr().read().dr() as u8;
        }
        wait_bsy(regs, &mut timeout)
    }

    /// Blocking full-duplex transfer (Master).
    pub fn blocking_transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_u8(regs, write, read, &mut timeout)
    }

    /// Blocking in-place transfer (Master).
    pub fn blocking_transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_in_place_u8(regs, words, &mut timeout)
    }

    /// Wait until the bus is idle.
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)
    }

    // --- 16-bit blocking methods ---

    /// Blocking write (Master, 16-bit).
    pub fn blocking_write_u16(&mut self, words: &[u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();

        if self.miso.is_some() {
            for &w in words {
                wait_txe(regs, &mut timeout)?;
                regs.dr().write(|d| d.set_dr(w));
                wait_rxne(regs, &mut timeout)?;
                let _ = regs.dr().read();
            }
            wait_bsy(regs, &mut timeout)?;
            let _ = regs.dr().read();
            let _ = regs.sr().read();
            Ok(())
        } else {
            for &w in words {
                wait_txe(regs, &mut timeout)?;
                regs.dr().write(|d| d.set_dr(w));
            }
            wait_bsy(regs, &mut timeout)?;
            Ok(())
        }
    }

    /// Blocking read (Master, 16-bit). Sends 0xFFFF dummy words to generate clock.
    pub fn blocking_read_u16(&mut self, words: &mut [u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        for b in words.iter_mut() {
            wait_txe(regs, &mut timeout)?;
            regs.dr().write(|d| d.set_dr(0xFFFF));
            wait_rxne(regs, &mut timeout)?;
            *b = regs.dr().read().dr();
        }
        wait_bsy(regs, &mut timeout)
    }

    /// Blocking full-duplex transfer (Master, 16-bit).
    pub fn blocking_transfer_u16(&mut self, read: &mut [u16], write: &[u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_u16(regs, write, read, &mut timeout)
    }

    /// Blocking in-place transfer (Master, 16-bit).
    pub fn blocking_transfer_in_place_u16(&mut self, words: &mut [u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_in_place_u16(regs, words, &mut timeout)
    }
}

// ====================
// Blocking transfer methods (Slave)
// ====================

impl<'d, M: Mode> Spi<'d, M, Slave> {
    /// Blocking write (Slave).
    pub fn blocking_write(&mut self, words: &[u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();

        if let Some((first, rest)) = words.split_first() {
            // Pre-load first byte before master starts clock
            wait_txe(regs, &mut timeout)?;
            regs.dr().write(|d| d.set_dr(*first as u16));

            for &w in rest {
                wait_txe(regs, &mut timeout)?;
                regs.dr().write(|d| d.set_dr(w as u16));
                // Read DR to prevent overrun (full-duplex slave always receives)
                // and check for overflow.
                let sr = regs.sr().read();
                if sr.ovr() {
                    let _ = regs.dr().read();
                    let _ = regs.sr().read();
                    return Err(Error::Overrun);
                }
                let _ = regs.dr().read();
            }

            wait_bsy(regs, &mut timeout)?;

            // Clear any residual overrun flag
            let _ = regs.dr().read();
            let _ = regs.sr().read();
        }
        Ok(())
    }

    /// Blocking read (Slave). Does NOT write to DR.
    pub fn blocking_read(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        for b in words.iter_mut() {
            wait_rxne(regs, &mut timeout)?;
            *b = regs.dr().read().dr() as u8;
        }
        Ok(())
    }

    /// Blocking full-duplex transfer (Slave).
    pub fn blocking_transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_u8(regs, write, read, &mut timeout)
    }

    /// Blocking in-place transfer (Slave).
    pub fn blocking_transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_in_place_u8(regs, words, &mut timeout)
    }

    /// Wait until the bus is idle.
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)
    }

    // --- 16-bit blocking methods ---

    /// Blocking write (Slave, 16-bit).
    pub fn blocking_write_u16(&mut self, words: &[u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();

        if let Some((first, rest)) = words.split_first() {
            wait_txe(regs, &mut timeout)?;
            regs.dr().write(|d| d.set_dr(*first));

            for &w in rest {
                wait_txe(regs, &mut timeout)?;
                regs.dr().write(|d| d.set_dr(w));
                let sr = regs.sr().read();
                if sr.ovr() {
                    let _ = regs.dr().read();
                    let _ = regs.sr().read();
                    return Err(Error::Overrun);
                }
                let _ = regs.dr().read();
            }

            wait_bsy(regs, &mut timeout)?;

            let _ = regs.dr().read();
            let _ = regs.sr().read();
        }
        Ok(())
    }

    /// Blocking read (Slave, 16-bit). Does NOT write to DR.
    pub fn blocking_read_u16(&mut self, words: &mut [u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        for b in words.iter_mut() {
            wait_rxne(regs, &mut timeout)?;
            *b = regs.dr().read().dr();
        }
        Ok(())
    }

    /// Blocking full-duplex transfer (Slave, 16-bit).
    pub fn blocking_transfer_u16(&mut self, read: &mut [u16], write: &[u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_u16(regs, write, read, &mut timeout)
    }

    /// Blocking in-place transfer (Slave, 16-bit).
    pub fn blocking_transfer_in_place_u16(&mut self, words: &mut [u16]) -> Result<(), Error> {
        let regs = self.info.regs;
        let mut timeout = self.timeout_ctx();
        transfer_in_place_u16(regs, words, &mut timeout)
    }
}

// ====================
// embedded-hal-0.2 impls (Master only)
// ====================

impl<'d> embedded_hal_02::blocking::spi::Transfer<u8> for Spi<'d, Blocking, Master> {
    type Error = Error;

    fn transfer<'w>(&mut self, words: &'w mut [u8]) -> Result<&'w [u8], Self::Error> {
        self.blocking_transfer_in_place(words)?;
        Ok(words)
    }
}

impl<'d> embedded_hal_02::blocking::spi::Write<u8> for Spi<'d, Blocking, Master> {
    type Error = Error;

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(words)
    }
}

impl<'d> embedded_hal_02::blocking::spi::Transfer<u16> for Spi<'d, Blocking, Master> {
    type Error = Error;

    fn transfer<'w>(&mut self, words: &'w mut [u16]) -> Result<&'w [u16], Self::Error> {
        self.blocking_transfer_in_place_u16(words)?;
        Ok(words)
    }
}

impl<'d> embedded_hal_02::blocking::spi::Write<u16> for Spi<'d, Blocking, Master> {
    type Error = Error;

    fn write(&mut self, words: &[u16]) -> Result<(), Self::Error> {
        self.blocking_write_u16(words)
    }
}

// ====================
// embedded-hal-1.0 impls
// ====================

impl embedded_hal_1::spi::Error for Error {
    fn kind(&self) -> embedded_hal_1::spi::ErrorKind {
        match *self {
            Self::Overrun => embedded_hal_1::spi::ErrorKind::Overrun,
            Self::ModeFault => embedded_hal_1::spi::ErrorKind::ModeFault,
            Self::Timeout => embedded_hal_1::spi::ErrorKind::Other,
            Self::Crc => embedded_hal_1::spi::ErrorKind::FrameFormat,
        }
    }
}

impl<'d, M: Mode, CM: CommunicationMode> embedded_hal_1::spi::ErrorType for Spi<'d, M, CM> {
    type Error = Error;
}

impl<'d> embedded_hal_1::spi::SpiBus<u8> for Spi<'d, Blocking, Master> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(words)
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(words)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.blocking_transfer(read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_transfer_in_place(words)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

impl<'d> embedded_hal_1::spi::SpiBus<u16> for Spi<'d, Blocking, Master> {
    fn read(&mut self, words: &mut [u16]) -> Result<(), Self::Error> {
        self.blocking_read_u16(words)
    }

    fn write(&mut self, words: &[u16]) -> Result<(), Self::Error> {
        self.blocking_write_u16(words)
    }

    fn transfer(&mut self, read: &mut [u16], write: &[u16]) -> Result<(), Self::Error> {
        self.blocking_transfer_u16(read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u16]) -> Result<(), Self::Error> {
        self.blocking_transfer_in_place_u16(words)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

// ====================
// DMA-based async methods (Master)
// ====================

#[cfg(dma)]
impl<'d> Spi<'d, Async, Master> {
    /// Async write using DMA.
    pub async fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let regs = self.info.regs;
        let dr_ptr = regs.dr().as_ptr() as *mut u8;

        regs.cr2().modify(|w| w.set_txdmaen(true));

        let tx_transfer = unsafe {
            self.tx_dma
                .as_mut()
                .unwrap()
                .write(data, dr_ptr, Default::default())
        };

        tx_transfer.await;

        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)?;

        regs.cr2().modify(|w| w.set_txdmaen(false));

        // Clear RX FIFO
        while regs.sr().read().rxne() {
            let _ = regs.dr().read();
        }

        Ok(())
    }

    /// Async read using DMA.
    ///
    /// Sends dummy bytes (0xFF) via TX DMA repeated-transfer to generate
    /// SCK clock, while RX DMA captures incoming data into the provided buffer.
    /// This matches the C HAL approach in `HAL_SPI_Receive_DMA` which
    /// internally calls `HAL_SPI_TransmitReceive_DMA` with the same buffer
    /// for TX and RX.
    pub async fn read(&mut self, data: &mut [u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let regs = self.info.regs;
        let dr_ptr = regs.dr().as_ptr() as *mut u8;

        // Enable both TX and RX DMA
        regs.cr2().modify(|w| {
            w.set_txdmaen(true);
            w.set_rxdmaen(true);
        });

        // Use write_repeated to send dummy 0xFF bytes for clock generation,
        // while RX DMA receives data into the buffer.
        // Using write_repeated avoids borrowing the same buffer for both TX and RX.
        let dummy = 0xFFu8;
        let tx = unsafe {
            self.tx_dma
                .as_mut()
                .unwrap()
                .write_repeated(&dummy, data.len(), dr_ptr, Default::default())
        };
        let rx = unsafe {
            self.rx_dma
                .as_mut()
                .unwrap()
                .read(dr_ptr, data, Default::default())
        };

        embassy_futures::join::join(tx, rx).await;

        // Wait for the last frame to complete, then drain RX FIFO.
        // Matches C HAL's SPI_EndRxTxTransaction() which waits FTLVL=0 → BSY=0 → FRLVL=0.
        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)?;
        while regs.sr().read().rxne() {
            let _ = regs.dr().read();
        }

        regs.cr2().modify(|w| {
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });

        Ok(())
    }

    /// Async full-duplex transfer using DMA.
    pub async fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        if read.len() != write.len() {
            return self.blocking_transfer(read, write);
        }

        let regs = self.info.regs;
        let dr_ptr = regs.dr().as_ptr() as *mut u8;

        regs.cr2().modify(|w| {
            w.set_txdmaen(true);
            w.set_rxdmaen(true);
        });

        let tx = unsafe { self.tx_dma.as_mut().unwrap().write(write, dr_ptr, Default::default()) };
        let rx = unsafe { self.rx_dma.as_mut().unwrap().read(dr_ptr, read, Default::default()) };

        embassy_futures::join::join(tx, rx).await;

        // Wait for the last frame to complete, then drain RX FIFO.
        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)?;
        while regs.sr().read().rxne() {
            let _ = regs.dr().read();
        }

        regs.cr2().modify(|w| {
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });

        Ok(())
    }

    // --- 16-bit DMA methods ---

    /// Async write using DMA (16-bit).
    pub async fn write_u16(&mut self, data: &[u16]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let regs = self.info.regs;
        let dr_ptr = regs.dr().as_ptr() as *mut u16;

        regs.cr2().modify(|w| w.set_txdmaen(true));

        let tx_transfer = unsafe {
            self.tx_dma
                .as_mut()
                .unwrap()
                .write(data, dr_ptr, Default::default())
        };

        tx_transfer.await;

        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)?;

        regs.cr2().modify(|w| w.set_txdmaen(false));

        while regs.sr().read().rxne() {
            let _ = regs.dr().read();
        }

        Ok(())
    }

    /// Async read using DMA (16-bit).
    pub async fn read_u16(&mut self, data: &mut [u16]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let regs = self.info.regs;
        let dr_ptr = regs.dr().as_ptr() as *mut u16;

        regs.cr2().modify(|w| {
            w.set_txdmaen(true);
            w.set_rxdmaen(true);
        });

        let dummy = 0xFFFFu16;
        let tx = unsafe {
            self.tx_dma
                .as_mut()
                .unwrap()
                .write_repeated(&dummy, data.len(), dr_ptr, Default::default())
        };
        let rx = unsafe {
            self.rx_dma
                .as_mut()
                .unwrap()
                .read(dr_ptr, data, Default::default())
        };

        embassy_futures::join::join(tx, rx).await;

        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)?;
        while regs.sr().read().rxne() {
            let _ = regs.dr().read();
        }

        regs.cr2().modify(|w| {
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });

        Ok(())
    }

    /// Async full-duplex transfer using DMA (16-bit).
    pub async fn transfer_u16(&mut self, read: &mut [u16], write: &[u16]) -> Result<(), Error> {
        if read.len() != write.len() {
            return self.blocking_transfer_u16(read, write);
        }

        let regs = self.info.regs;
        let dr_ptr = regs.dr().as_ptr() as *mut u16;

        regs.cr2().modify(|w| {
            w.set_txdmaen(true);
            w.set_rxdmaen(true);
        });

        let tx = unsafe { self.tx_dma.as_mut().unwrap().write(write, dr_ptr, Default::default()) };
        let rx = unsafe { self.rx_dma.as_mut().unwrap().read(dr_ptr, read, Default::default()) };

        embassy_futures::join::join(tx, rx).await;

        let mut timeout = self.timeout_ctx();
        wait_bsy(regs, &mut timeout)?;
        while regs.sr().read().rxne() {
            let _ = regs.dr().read();
        }

        regs.cr2().modify(|w| {
            w.set_txdmaen(false);
            w.set_rxdmaen(false);
        });

        Ok(())
    }
}

#[cfg(dma)]
impl<'d> embedded_hal_async::spi::SpiBus<u8> for Spi<'d, Async, Master> {
    async fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.read(words).await
    }

    async fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.write(words).await
    }

    async fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.transfer(read, write).await
    }

    async fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_transfer_in_place(words)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

#[cfg(dma)]
impl<'d> embedded_hal_async::spi::SpiBus<u16> for Spi<'d, Async, Master> {
    async fn read(&mut self, words: &mut [u16]) -> Result<(), Self::Error> {
        self.read_u16(words).await
    }

    async fn write(&mut self, words: &[u16]) -> Result<(), Self::Error> {
        self.write_u16(words).await
    }

    async fn transfer(&mut self, read: &mut [u16], write: &[u16]) -> Result<(), Self::Error> {
        self.transfer_u16(read, write).await
    }

    async fn transfer_in_place(&mut self, words: &mut [u16]) -> Result<(), Self::Error> {
        self.blocking_transfer_in_place_u16(words)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}
