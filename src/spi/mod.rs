//! Serial Peripheral Interface (SPI)
#![macro_use]

use core::marker::PhantomData;
use core::ptr;

use embassy_embedded_hal::SetConfig;
use embassy_futures::join::join;
use embassy_hal_internal::{Peripheral, PeripheralRef};
pub use embedded_hal_02::spi::{MODE_0, MODE_1, MODE_2, MODE_3, Mode, Phase, Polarity};

use crate::dma::{ChannelAndRequest, word};
use crate::gpio::{AfType, AnyPin, OutputType, Pin, Pull, SealedPin, Speed};
use crate::mode::{Async, Blocking, Mode as PeriMode};
use crate::pac::spi::{Spi as Regs, regs, vals};
use crate::rcc::{RccInfo, SealedRccPeripheral};
use crate::time::Hertz;

/// SPI error.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Invalid framing.
    Framing,
    /// CRC error (only if hardware CRC checking is enabled).
    Crc,
    /// Mode fault
    ModeFault,
    /// Overrun.
    Overrun,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Self::Framing => "Invalid Framing",
            Self::Crc => "Hardware CRC Check Failed",
            Self::ModeFault => "Mode Fault",
            Self::Overrun => "Buffer Overrun",
        };

        write!(f, "{}", message)
    }
}

impl core::error::Error for Error {}

/// SPI bit order
#[derive(Copy, Clone)]
pub enum BitOrder {
    /// Least significant bit first.
    LsbFirst,
    /// Most significant bit first.
    MsbFirst,
}

/// SPI Direction.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Direction {
    /// Transmit
    Transmit,
    /// Receive
    Receive,
}

/// Slave Select (SS) pin polarity.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SlaveSelectPolarity {
    /// SS active high
    ActiveHigh,
    /// SS active low
    ActiveLow,
}

/// SPI configuration.
#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct Config {
    /// SPI mode.
    pub mode: Mode,
    /// Bit order.
    pub bit_order: BitOrder,
    /// Clock frequency.
    pub frequency: Hertz,
    /// Enable internal pullup on MISO.
    ///
    /// There are some ICs that require a pull-up on the MISO pin for some applications.
    /// If you  are unsure, you probably don't need this.
    pub miso_pull: Pull,
    /// signal rise/fall speed (slew rate) - defaults to `Medium`.
    /// Increase for high SPI speeds. Change to `Low` to reduce ringing.
    pub gpio_speed: Speed,
    /// If True sets SSOE to zero even if SPI is in Master Mode.
    /// NSS output enabled (SSM = 0, SSOE = 1): The NSS signal is driven low when the master starts the communication and is kept low until the SPI is disabled.
    /// NSS output disabled (SSM = 0, SSOE = 0): For devices set as slave, the NSS pin acts as a classical NSS input: the slave is selected when NSS is low and deselected when NSS high.
    pub nss_output_disable: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: MODE_0,
            bit_order: BitOrder::MsbFirst,
            frequency: Hertz(1_000_000),
            miso_pull: Pull::None,
            gpio_speed: Speed::VeryHigh,
            nss_output_disable: false,
        }
    }
}

impl Config {
    fn raw_phase(&self) -> vals::Cpha {
        match self.mode.phase {
            Phase::CaptureOnSecondTransition => vals::Cpha::SECONDEDGE,
            Phase::CaptureOnFirstTransition => vals::Cpha::FIRSTEDGE,
        }
    }

    fn raw_polarity(&self) -> vals::Cpol {
        match self.mode.polarity {
            Polarity::IdleHigh => vals::Cpol::IDLEHIGH,
            Polarity::IdleLow => vals::Cpol::IDLELOW,
        }
    }

    fn raw_byte_order(&self) -> vals::Lsbfirst {
        match self.bit_order {
            BitOrder::LsbFirst => vals::Lsbfirst::LSBFIRST,
            BitOrder::MsbFirst => vals::Lsbfirst::MSBFIRST,
        }
    }

    fn sck_af(&self) -> AfType {
        AfType::output_pull(
            OutputType::PushPull,
            self.gpio_speed,
            match self.mode.polarity {
                Polarity::IdleLow => Pull::Down,
                Polarity::IdleHigh => Pull::Up,
            },
        )
    }

    fn mosi_af(&self) -> AfType {
        AfType::output(
            OutputType::PushPull,
            self.gpio_speed
        )
    }

    fn miso_af(&self) -> AfType {
        AfType::output(
            OutputType::PushPull,
            self.gpio_speed
        )
    }

    fn nss_af(&self) -> AfType {
        AfType::output(
            OutputType::PushPull,
            self.gpio_speed
        )
    }
}

/// SPI communication mode
pub mod mode {
    use py32_metapac::spi::vals;

    trait SealedMode {}

    /// Trait for SPI communication mode operations.
    #[allow(private_bounds)]
    pub trait CommunicationMode: SealedMode {
        /// Spi communication mode
        const MASTER: vals::Mstr;
    }

    /// Mode allowing for SPI master operations.
    pub struct Master;
    /// Mode allowing for SPI slave operations.
    pub struct Slave;

    impl SealedMode for Master {}
    impl CommunicationMode for Master {
        const MASTER: vals::Mstr = vals::Mstr::MASTER;
    }

    impl SealedMode for Slave {}
    impl CommunicationMode for Slave {
        const MASTER: vals::Mstr = vals::Mstr::SLAVE;
    }
}
use mode::{CommunicationMode, Master, Slave};

/// SPI driver.
pub struct Spi<'d, M: PeriMode, CM: CommunicationMode> {
    pub(crate) info: &'static Info,
    kernel_clock: Hertz,
    sck: Option<PeripheralRef<'d, AnyPin>>,
    mosi: Option<PeripheralRef<'d, AnyPin>>,
    miso: Option<PeripheralRef<'d, AnyPin>>,
    nss: Option<PeripheralRef<'d, AnyPin>>,
    tx_dma: Option<ChannelAndRequest<'d>>,
    rx_dma: Option<ChannelAndRequest<'d>>,
    _phantom: PhantomData<(M, CM)>,
    current_word_size: word_impl::Config,
    gpio_speed: Speed,
}

impl<'d, M: PeriMode, CM: CommunicationMode> Spi<'d, M, CM> {
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        sck: Option<PeripheralRef<'d, AnyPin>>,
        mosi: Option<PeripheralRef<'d, AnyPin>>,
        miso: Option<PeripheralRef<'d, AnyPin>>,
        nss: Option<PeripheralRef<'d, AnyPin>>,
        tx_dma: Option<ChannelAndRequest<'d>>,
        rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Self {
        let mut this = Self {
            info: T::info(),
            kernel_clock: T::frequency(),
            sck,
            mosi,
            miso,
            nss,
            tx_dma,
            rx_dma,
            current_word_size: <u8 as SealedWord>::CONFIG,
            _phantom: PhantomData,
            gpio_speed: config.gpio_speed,
        };
        this.enable_and_init(config);
        this
    }

    fn enable_and_init(&mut self, config: Config) {
        let br = compute_baud_rate(self.kernel_clock, config.frequency);
        let cpha = config.raw_phase();
        let cpol = config.raw_polarity();
        let lsbfirst = config.raw_byte_order();

        self.info.rcc.enable_and_reset(); //_without_stop();

        /*
        - Software NSS management (SSM = 1)
        The slave select information is driven internally by the value of the SSI bit in the
        SPI_CR1 register. The external NSS pin remains free for other application uses.

        - Hardware NSS management (SSM = 0)
        Two configurations are possible depending on the NSS output configuration (SSOE bit
        in register SPI_CR1).

        -- NSS output enabled (SSM = 0, SSOE = 1)
          This configuration is used only when the device operates in master mode. The
          NSS signal is driven low when the master starts the communication and is kept
          low until the SPI is disabled.

        -- NSS output disabled (SSM = 0, SSOE = 0)
            This configuration allows multimaster capability for devices operating in master
            mode. For devices set as slave, the NSS pin acts as a classical NSS input: the
            slave is selected when NSS is low and deselected when NSS high
         */
        let ssm = self.nss.is_none();

        let regs = self.info.regs;
        let ssoe = CM::MASTER == vals::Mstr::MASTER && !config.nss_output_disable;
        regs.cr2().modify(|w| {
            w.set_ssoe(ssoe);
            w.set_ds(<u8 as SealedWord>::CONFIG);
        });
        regs.cr1().modify(|w| {
            w.set_cpha(cpha);
            w.set_cpol(cpol);

            w.set_mstr(CM::MASTER);
            w.set_br(br);
            w.set_spe(true);
            w.set_lsbfirst(lsbfirst);
            w.set_ssi(CM::MASTER == vals::Mstr::MASTER);
            w.set_ssm(ssm);
            w.set_bidimode(vals::Bidimode::UNIDIRECTIONAL);
            // we're doing "fake rxonly", by actually writing one
            // byte to TXDR for each byte we want to receive. if we
            // set OUTPUTDISABLED here, this hangs.
            w.set_rxonly(vals::Rxonly::FULLDUPLEX);
        });
    }

    /// Reconfigures it with the supplied config.
    pub fn set_config(&mut self, config: &Config) -> Result<(), ()> {
        let cpha = config.raw_phase();
        let cpol = config.raw_polarity();

        let lsbfirst = config.raw_byte_order();

        let br = compute_baud_rate(self.kernel_clock, config.frequency);

        self.gpio_speed = config.gpio_speed;
        let sck = self.sck;
        if let Some(sck) = self.sck {
            new_pin!(sck, config.sck_af());
        }
        if let Some(mosi) = self.mosi {
            new_pin!(mosi, config.mosi_af());
        }
        if let Some(miso) = self.miso {
            new_pin!(miso, config.miso_af());
        }
        if let Some(nss) = self.nss {
            if !config.nss_output_disable {
                new_pin!(nss, config.nss_af());
            }
        }
        self.info.regs.cr2().modify(|w| 
            w.set_slvfm(Br::DIV4 == br && CM::MASTER == vals::Mstr::SLAVE));

        self.info.regs.cr1().modify(|w| {
            w.set_cpha(cpha);
            w.set_cpol(cpol);
            w.set_br(br);
            w.set_lsbfirst(lsbfirst);
        });

        Ok(())
    }

    /// Set SPI direction
    pub fn set_direction(&mut self, dir: Option<Direction>) {
        let (bidimode, bidioe) = match dir {
            Some(Direction::Transmit) => (vals::Bidimode::BIDIRECTIONAL, vals::Bidioe::TRANSMIT),
            Some(Direction::Receive) => (vals::Bidimode::BIDIRECTIONAL, vals::Bidioe::RECEIVE),
            None => (vals::Bidimode::UNIDIRECTIONAL, vals::Bidioe::TRANSMIT),
        };
        self.info.regs.cr1().modify(|w| {
            w.set_bidimode(bidimode);
            w.set_bidioe(bidioe);
        });
    }

    /// Get current SPI configuration.
    pub fn get_current_config(&self) -> Config {
        let cfg = self.info.regs.cr1().read();

        let ssoe = self.info.regs.cr2().read().ssoe();

        let polarity = if cfg.cpol() == vals::Cpol::IDLELOW {
            Polarity::IdleLow
        } else {
            Polarity::IdleHigh
        };
        let phase = if cfg.cpha() == vals::Cpha::FIRSTEDGE {
            Phase::CaptureOnFirstTransition
        } else {
            Phase::CaptureOnSecondTransition
        };

        let bit_order = if cfg.lsbfirst() == vals::Lsbfirst::LSBFIRST {
            BitOrder::LsbFirst
        } else {
            BitOrder::MsbFirst
        };



        let miso_pull = match polarity {
            Polarity::IdleLow => Pull::Down,
            Polarity::IdleHigh => Pull::Up,
        };

        let br = cfg.br();

        let frequency = compute_frequency(self.kernel_clock, br);

        // NSS output disabled if SSOE=0 or if SSM=1 software slave management enabled
        let nss_output_disable = !ssoe || cfg.ssm();

        Config {
            mode: Mode { polarity, phase },
            bit_order,
            frequency,
            miso_pull,
            gpio_speed: self.gpio_speed,
            nss_output_disable,
        }
    }

    pub(crate) fn set_word_size(&mut self, word_size: word_impl::Config) {
        if self.current_word_size == word_size {
            return;
        }

        self.info.regs.cr1().modify(|w| {
            w.set_spe(false);
        });
        self.info.regs.cr2().modify(|reg| {
            reg.set_ds(word_size);
        });

        self.current_word_size = word_size;
    }

    /// Blocking write.
    pub fn blocking_write<W: Word>(&mut self, words: &[W]) -> Result<(), Error> {
        self.set_word_size(W::CONFIG);
        self.info.regs.cr1().modify(|w| w.set_spe(true));
        flush_rx_fifo(self.info.regs);
        for word in words.iter() {
            transfer_word(self.info.regs, *word)?;
        }
        Ok(())
    }

    /// Blocking read.
    pub fn blocking_read<W: Word>(&mut self, words: &mut [W]) -> Result<(), Error> {
        self.set_word_size(W::CONFIG);
        self.info.regs.cr1().modify(|w| w.set_spe(true));
        flush_rx_fifo(self.info.regs);
        for word in words.iter_mut() {
            *word = transfer_word(self.info.regs, W::default())?;
        }
        Ok(())
    }

    /// Blocking in-place bidirectional transfer.
    ///
    /// This writes the contents of `data` on MOSI, and puts the received data on MISO in `data`, at the same time.
    pub fn blocking_transfer_in_place<W: Word>(&mut self, words: &mut [W]) -> Result<(), Error> {
        self.set_word_size(W::CONFIG);
        self.info.regs.cr1().modify(|w| w.set_spe(true));
        flush_rx_fifo(self.info.regs);
        for word in words.iter_mut() {
            *word = transfer_word(self.info.regs, *word)?;
        }
        Ok(())
    }

    /// Blocking bidirectional transfer.
    ///
    /// This transfers both buffers at the same time, so it is NOT equivalent to `write` followed by `read`.
    ///
    /// The transfer runs for `max(read.len(), write.len())` bytes. If `read` is shorter extra bytes are ignored.
    /// If `write` is shorter it is padded with zero bytes.
    pub fn blocking_transfer<W: Word>(&mut self, read: &mut [W], write: &[W]) -> Result<(), Error> {
        self.set_word_size(W::CONFIG);
        self.info.regs.cr1().modify(|w| w.set_spe(true));
        flush_rx_fifo(self.info.regs);
        let len = read.len().max(write.len());
        for i in 0..len {
            let wb = write.get(i).copied().unwrap_or_default();
            let rb = transfer_word(self.info.regs, wb)?;
            if let Some(r) = read.get_mut(i) {
                *r = rb;
            }
        }
        Ok(())
    }
}

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
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            new_pin!(miso, AfType::input(config.miso_pull)),
            new_pin!(cs, AfType::input(Pull::None)),
            None,
            None,
            config,
        )
    }
}

impl<'d> Spi<'d, Blocking, Master> {
    /// Create a new blocking SPI driver.
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
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            new_pin!(miso, AfType::input(config.miso_pull)),
            None,
            None,
            None,
            config,
        )
    }

    /// Create a new blocking SPI driver, in RX-only mode (only MISO pin, no MOSI).
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
            new_pin!(miso, AfType::input(config.miso_pull)),
            None,
            None,
            None,
            config,
        )
    }

    /// Create a new blocking SPI driver, in TX-only mode (only MOSI pin, no MISO).
    pub fn new_blocking_txonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            None,
            None,
            None,
            None,
            config,
        )
    }

    /// Create a new SPI driver, in TX-only mode, without SCK pin.
    ///
    /// This can be useful for bit-banging non-SPI protocols.
    pub fn new_blocking_txonly_nosck<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            None,
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            None,
            None,
            None,
            None,
            config,
        )
    }
}

impl<'d> Spi<'d, Async, Slave> {
    /// Create a new SPI slave driver.
    pub fn new_slave<T: Instance>(
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
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            new_pin!(miso, AfType::input(config.miso_pull)),
            new_pin!(cs, AfType::input(Pull::None)),
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        )
    }
}

impl<'d> Spi<'d, Async, Master> {
    /// Create a new SPI driver.
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
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            new_pin!(miso, AfType::input(config.miso_pull)),
            None,
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        )
    }

    /// Create a new SPI driver, in RX-only mode (only MISO pin, no MOSI).
    pub fn new_rxonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        rx_dma: impl Peripheral<P = impl RxDma<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            None,
            new_pin!(miso, AfType::input(config.miso_pull)),
            None,
            None,
            new_dma!(rx_dma),
            config,
        )
    }

    /// Create a new SPI driver, in TX-only mode (only MOSI pin, no MISO).
    pub fn new_txonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            None,
            None,
            new_dma!(tx_dma),
            None,
            config,
        )
    }

    /// Create a new SPI driver, in bidirectional mode, specifically in tranmit mode
    #[cfg(any(spi_v1, spi_v2, spi_v3))]
    pub fn new_bidi<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        sck: impl Peripheral<P = impl SckPin<T>> + 'd,
        sdio: impl Peripheral<P = impl MosiPin<T>> + 'd,
        tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        rx_dma: impl Peripheral<P = impl RxDma<T>> + 'd,
        config: Config,
    ) -> Self {
        let mut this = Self::new_inner(
            peri,
            new_pin!(sck, config.sck_af()),
            new_pin!(sdio, AfType::output(OutputType::PushPull, config.gpio_speed)),
            None,
            None,
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        );
        this.set_direction(Some(Direction::Transmit));
        this
    }

    /// Create a new SPI driver, in TX-only mode, without SCK pin.
    ///
    /// This can be useful for bit-banging non-SPI protocols.
    pub fn new_txonly_nosck<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            None,
            new_pin!(mosi, AfType::output(OutputType::PushPull, config.gpio_speed)),
            None,
            None,
            new_dma!(tx_dma),
            None,
            config,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn new_internal<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        tx_dma: Option<ChannelAndRequest<'d>>,
        rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Self {
        Self::new_inner(peri, None, None, None, None, tx_dma, rx_dma, config)
    }
}

impl<'d, CM: CommunicationMode> Spi<'d, Async, CM> {
    /// SPI write, using DMA.
    pub async fn write<W: Word>(&mut self, data: &[W]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        self.info.regs.cr1().modify(|w| {
            w.set_spe(false);
        });
        self.set_word_size(W::CONFIG);

        let tx_dst = self.info.regs.tx_ptr();
        let tx_f = unsafe { self.tx_dma.as_mut().unwrap().write(data, tx_dst, Default::default()) };

        set_txdmaen(self.info.regs, true);
        self.info.regs.cr1().modify(|w| {
            w.set_spe(true);
        });

        tx_f.await;

        finish_dma(self.info.regs);

        Ok(())
    }

    /// SPI read, using DMA.
    pub async fn read<W: Word>(&mut self, data: &mut [W]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        self.info.regs.cr1().modify(|w| {
            w.set_spe(false);
        });

        self.set_word_size(W::CONFIG);

        flush_rx_fifo(self.info.regs);

        set_rxdmaen(self.info.regs, true);

        let clock_byte_count = data.len();

        let rx_src = self.info.regs.rx_ptr();
        let rx_f = unsafe { self.rx_dma.as_mut().unwrap().read(rx_src, data, Default::default()) };

        let tx_dst = self.info.regs.tx_ptr();
        let clock_byte = W::default();
        let tx_f = unsafe {
            self.tx_dma
                .as_mut()
                .unwrap()
                .write_repeated(&clock_byte, clock_byte_count, tx_dst, Default::default())
        };

        set_txdmaen(self.info.regs, true);
        self.info.regs.cr1().modify(|w| {
            w.set_spe(true);
        });

        join(tx_f, rx_f).await;

        finish_dma(self.info.regs);

        Ok(())
    }

    async fn transfer_inner<W: Word>(&mut self, read: *mut [W], write: *const [W]) -> Result<(), Error> {
        assert_eq!(read.len(), write.len());
        if read.len() == 0 {
            return Ok(());
        }

        self.info.regs.cr1().modify(|w| {
            w.set_spe(false);
        });

        self.set_word_size(W::CONFIG);

        // SPIv3 clears rxfifo on SPE=0
        #[cfg(not(any(spi_v4, spi_v5, spi_v6)))]
        flush_rx_fifo(self.info.regs);

        set_rxdmaen(self.info.regs, true);

        let rx_src = self.info.regs.rx_ptr::<W>();
        let rx_f = unsafe { self.rx_dma.as_mut().unwrap().read_raw(rx_src, read, Default::default()) };

        let tx_dst: *mut W = self.info.regs.tx_ptr();
        let tx_f = unsafe {
            self.tx_dma
                .as_mut()
                .unwrap()
                .write_raw(write, tx_dst, Default::default())
        };

        set_txdmaen(self.info.regs, true);
        self.info.regs.cr1().modify(|w| {
            w.set_spe(true);
        });

        join(tx_f, rx_f).await;

        finish_dma(self.info.regs);

        Ok(())
    }

    /// Bidirectional transfer, using DMA.
    ///
    /// This transfers both buffers at the same time, so it is NOT equivalent to `write` followed by `read`.
    ///
    /// The transfer runs for `max(read.len(), write.len())` bytes. If `read` is shorter extra bytes are ignored.
    /// If `write` is shorter it is padded with zero bytes.
    pub async fn transfer<W: Word>(&mut self, read: &mut [W], write: &[W]) -> Result<(), Error> {
        self.transfer_inner(read, write).await
    }

    /// In-place bidirectional transfer, using DMA.
    ///
    /// This writes the contents of `data` on MOSI, and puts the received data on MISO in `data`, at the same time.
    pub async fn transfer_in_place<W: Word>(&mut self, data: &mut [W]) -> Result<(), Error> {
        self.transfer_inner(data, data).await
    }
}

impl<'d, M: PeriMode, CM: CommunicationMode> Drop for Spi<'d, M, CM> {
    fn drop(&mut self) {
        self.sck.as_ref().map(|x| x.set_as_disconnected());
        self.mosi.as_ref().map(|x| x.set_as_disconnected());
        self.miso.as_ref().map(|x| x.set_as_disconnected());
        self.nss.as_ref().map(|x| x.set_as_disconnected());
        self.info.rcc.disable();
    }
}

use vals::Br;

fn compute_baud_rate(kernel_clock: Hertz, freq: Hertz) -> Br {
    let val = match kernel_clock.0 / freq.0 {
        0 => panic!("You are trying to reach a frequency higher than the clock"),
        1..=2 => 0b000,
        3..=5 => 0b001,
        6..=11 => 0b010,
        12..=23 => 0b011,
        24..=39 => 0b100,
        40..=95 => 0b101,
        96..=191 => 0b110,
        _ => 0b111,
    };

    Br::from_bits(val)
}

fn compute_frequency(kernel_clock: Hertz, br: Br) -> Hertz {
    let div: u16 = match br {
        Br::DIV2 => 2,
        Br::DIV4 => 4,
        Br::DIV8 => 8,
        Br::DIV16 => 16,
        Br::DIV32 => 32,
        Br::DIV64 => 64,
        Br::DIV128 => 128,
        Br::DIV256 => 256,
    };

    kernel_clock / div
}

pub(crate) trait RegsExt {
    fn tx_ptr<W>(&self) -> *mut W;
    fn rx_ptr<W>(&self) -> *mut W;
}

impl RegsExt for Regs {
    fn tx_ptr<W>(&self) -> *mut W {
        let dr = self.dr();
        dr.as_ptr() as *mut W
    }

    fn rx_ptr<W>(&self) -> *mut W {
        let dr = self.dr();
        dr.as_ptr() as *mut W
    }
}

fn check_error_flags(sr: regs::Sr, ovr: bool) -> Result<(), Error> {
    if sr.ovr() && ovr {
        return Err(Error::Overrun);
    }
    if sr.modf() {
        return Err(Error::ModeFault);
    }
    Ok(())
}

fn spin_until_tx_ready(regs: Regs, ovr: bool) -> Result<(), Error> {
    loop {
        let sr = regs.sr().read();

        check_error_flags(sr, ovr)?;

        if sr.txe() {
            return Ok(());
        }
    }
}

fn spin_until_rx_ready(regs: Regs) -> Result<(), Error> {
    loop {
        let sr = regs.sr().read();

        check_error_flags(sr, true)?;

        if sr.rxne() {
            return Ok(());
        }
    }
}

pub(crate) fn flush_rx_fifo(regs: Regs) {
    while regs.sr().read().rxne() {
        let _ = regs.dr().read();
    }
}

pub(crate) fn set_txdmaen(regs: Regs, val: bool) {
    regs.cr2().modify(|reg| {
        reg.set_txdmaen(val);
    });
}

pub(crate) fn set_rxdmaen(regs: Regs, val: bool) {
    regs.cr2().modify(|reg| {
        reg.set_rxdmaen(val);
    });
}

fn finish_dma(regs: Regs) {
    while regs.sr().read().bsy() {}

    // Disable the spi peripheral
    regs.cr1().modify(|w| {
        w.set_spe(false);
    });

    // The peripheral automatically disables the DMA stream on completion without error,
    // but it does not clear the RXDMAEN/TXDMAEN flag in CR2.
    regs.cr2().modify(|reg| {
        reg.set_txdmaen(false);
        reg.set_rxdmaen(false);
    });
}

fn transfer_word<W: Word>(regs: Regs, tx_word: W) -> Result<W, Error> {
    spin_until_tx_ready(regs, true)?;

    unsafe {
        ptr::write_volatile(regs.tx_ptr(), tx_word);
    }

    spin_until_rx_ready(regs)?;

    let rx_word = unsafe { ptr::read_volatile(regs.rx_ptr()) };
    Ok(rx_word)
}

#[allow(unused)] // unused in SPIv1
fn write_word<W: Word>(regs: Regs, tx_word: W) -> Result<(), Error> {
    // for write, we intentionally ignore the rx fifo, which will cause
    // overrun errors that we have to ignore.
    spin_until_tx_ready(regs, false)?;

    unsafe {
        ptr::write_volatile(regs.tx_ptr(), tx_word);
    }
    Ok(())
}

// Note: It is not possible to impl these traits generically in embedded-hal 0.2 due to a conflict with
// some marker traits. For details, see https://github.com/rust-embedded/embedded-hal/pull/289
macro_rules! impl_blocking {
    ($w:ident) => {
        impl<'d, M: PeriMode, CM: CommunicationMode> embedded_hal_02::blocking::spi::Write<$w> for Spi<'d, M, CM> {
            type Error = Error;

            fn write(&mut self, words: &[$w]) -> Result<(), Self::Error> {
                self.blocking_write(words)
            }
        }

        impl<'d, M: PeriMode, CM: CommunicationMode> embedded_hal_02::blocking::spi::Transfer<$w> for Spi<'d, M, CM> {
            type Error = Error;

            fn transfer<'w>(&mut self, words: &'w mut [$w]) -> Result<&'w [$w], Self::Error> {
                self.blocking_transfer_in_place(words)?;
                Ok(words)
            }
        }
    };
}

impl_blocking!(u8);
impl_blocking!(u16);

impl<'d, M: PeriMode, CM: CommunicationMode> embedded_hal_1::spi::ErrorType for Spi<'d, M, CM> {
    type Error = Error;
}

impl<'d, W: Word, M: PeriMode, CM: CommunicationMode> embedded_hal_1::spi::SpiBus<W> for Spi<'d, M, CM> {
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn read(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.blocking_read(words)
    }

    fn write(&mut self, words: &[W]) -> Result<(), Self::Error> {
        self.blocking_write(words)
    }

    fn transfer(&mut self, read: &mut [W], write: &[W]) -> Result<(), Self::Error> {
        self.blocking_transfer(read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.blocking_transfer_in_place(words)
    }
}

impl embedded_hal_1::spi::Error for Error {
    fn kind(&self) -> embedded_hal_1::spi::ErrorKind {
        match *self {
            Self::Framing => embedded_hal_1::spi::ErrorKind::FrameFormat,
            Self::Crc => embedded_hal_1::spi::ErrorKind::Other,
            Self::ModeFault => embedded_hal_1::spi::ErrorKind::ModeFault,
            Self::Overrun => embedded_hal_1::spi::ErrorKind::Overrun,
        }
    }
}

impl<'d, W: Word, CM: CommunicationMode> embedded_hal_async::spi::SpiBus<W> for Spi<'d, Async, CM> {
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn write(&mut self, words: &[W]) -> Result<(), Self::Error> {
        self.write(words).await
    }

    async fn read(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.read(words).await
    }

    async fn transfer(&mut self, read: &mut [W], write: &[W]) -> Result<(), Self::Error> {
        self.transfer(read, write).await
    }

    async fn transfer_in_place(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.transfer_in_place(words).await
    }
}

pub(crate) trait SealedWord {
    const CONFIG: word_impl::Config;
}

/// Word sizes usable for SPI.
#[allow(private_bounds)]
pub trait Word: word::Word + SealedWord + Default {}

macro_rules! impl_word {
    ($T:ty, $config:expr) => {
        impl SealedWord for $T {
            const CONFIG: Config = $config;
        }
        impl Word for $T {}
    };
}

#[cfg(any(spi_v1, spi_v2))]
mod word_impl {
    use super::*;

    pub type Config = bool;

    impl_word!(u8, false);
    impl_word!(u16, true);
}

pub(crate) struct Info {
    pub(crate) regs: Regs,
    pub(crate) rcc: RccInfo,
}

struct State {}

impl State {
    #[allow(unused)]
    const fn new() -> Self {
        Self {}
    }
}

peri_trait!();

pin_trait!(SckPin, Instance);
pin_trait!(MosiPin, Instance);
pin_trait!(MisoPin, Instance);
pin_trait!(CsPin, Instance);
pin_trait!(MckPin, Instance);
pin_trait!(CkPin, Instance);
pin_trait!(WsPin, Instance);
dma_trait!(RxDma, Instance);
dma_trait!(TxDma, Instance);

foreach_peripheral!(
    (spi, $inst:ident) => {
        peri_trait_impl!($inst, Info {
            regs: crate::pac::$inst,
            rcc: crate::peripherals::$inst::RCC_INFO,
        });
    };
);

impl<'d, M: PeriMode, CM: CommunicationMode> SetConfig for Spi<'d, M, CM> {
    type Config = Config;
    type ConfigError = ();
    fn set_config(&mut self, config: &Self::Config) -> Result<(), ()> {
        self.set_config(config)
    }
}