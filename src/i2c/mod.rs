//! Inter-Integrated-Circuit (I2C)
#![macro_use]

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

mod v1;

use core::future::Future;
use core::iter;
use core::marker::PhantomData;

use embassy_hal_internal::{Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;
#[cfg(feature = "time")]
use embassy_time::{Duration, Instant};

#[cfg(dma)]
use crate::dma::ChannelAndRequest;
use crate::gpio::{AfType, AnyPin, OutputType, SealedPin as _, Speed};
use crate::i2c::mode::{Master, MultiMaster};
use crate::interrupt::typelevel::Interrupt;
use crate::mode::{Async, Blocking, Mode};
use crate::rcc::{RccInfo, SealedRccPeripheral};
use crate::time::Hertz;
use crate::{interrupt, peripherals};

/// I2C modes
pub mod mode {
    trait SealedMode {}

    /// Trait for I2C master operations.
    #[allow(private_bounds)]
    pub trait MasterMode: SealedMode {}

    /// Mode allowing for I2C master operations.
    pub struct Master;
    /// Mode allowing for I2C master and slave operations.
    pub struct MultiMaster;

    impl SealedMode for Master {}
    impl MasterMode for Master {}

    impl SealedMode for MultiMaster {}
    impl MasterMode for MultiMaster {}
}
use mode::{MasterMode};

/// I2C error.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Bus error
    Bus,
    /// Arbitration lost
    Arbitration,
    /// ACK not received (either to the address or to a data byte)
    Nack,
    /// Timeout
    Timeout,
    /// CRC error
    Crc,
    /// Overrun error
    Overrun,
    /// Zero-length transfers are not allowed.
    ZeroLengthTransfer,
    /// Bus busy when trying to transmit
    Busy
}

/// I2C config
#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct Config {
    // /// Enable internal pullup on SDA.
    // ///
    // /// Using external pullup resistors is recommended for I2C. If you do
    // /// have external pullups you should not enable this.
    // pub sda_pullup: bool,
    // /// Enable internal pullup on SCL.
    // ///
    // /// Using external pullup resistors is recommended for I2C. If you do
    // /// have external pullups you should not enable this.
    // pub scl_pullup: bool,
    /// Timeout.
    #[cfg(feature = "time")]
    pub timeout: embassy_time::Duration,
}

pub struct SlaveAddrConfig {
    pub gencall : bool,
    pub own_addr : u8
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // sda_pullup: false,
            // scl_pullup: false,
            #[cfg(feature = "time")]
            timeout: embassy_time::Duration::from_millis(1000),
        }
    }
}

impl Config {
    fn scl_af(&self) -> AfType {
        return AfType::output(OutputType::OpenDrain, Speed::VeryHigh);
        // return AfType::output_pull(
        //     OutputType::OpenDrain,
        //     Speed::Medium,
        //     match self.scl_pullup {
        //         true => Pull::Up,
        //         false => Pull::None,
        //     },
        // );
    }

    fn sda_af(&self) -> AfType {
        return AfType::output(OutputType::OpenDrain, Speed::VeryHigh);
        // return AfType::output_pull(
        //     OutputType::OpenDrain,
        //     Speed::Medium,
        //     match self.sda_pullup {
        //         true => Pull::Up,
        //         false => Pull::None,
        //     },
        // );
    }
}


struct I2CDropGuard<'d> {
    info: &'static Info,
    scl: Option<PeripheralRef<'d, AnyPin>>,
    sda: Option<PeripheralRef<'d, AnyPin>>,
}

impl<'d> Drop for I2CDropGuard<'d> {
    fn drop(&mut self) {
        self.scl.as_ref().map(|x| x.set_as_disconnected());
        self.sda.as_ref().map(|x| x.set_as_disconnected());

        self.info.rcc.disable();
    }
}

/// I2C driver.
pub struct I2c<'d, B:Mode, M: MasterMode> {
    info: &'static Info,
    #[allow(dead_code)]
    state: &'static State,
    kernel_clock: Hertz,
    #[cfg(dma)] tx_dma: Option<ChannelAndRequest<'d>>,
    #[cfg(dma)] rx_dma: Option<ChannelAndRequest<'d>>,
    #[cfg(feature = "time")]
    timeout: Duration,
    _drop_guard: I2CDropGuard<'d>,
    _phantom: PhantomData<M>,
    _phantom2: PhantomData<B>,
}

impl<'d> I2c<'d, Async, Master> {
    /// Create a new I2C driver.
    pub fn new<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        scl: impl Peripheral<P = impl SclPin<T>> + 'd,
        sda: impl Peripheral<P = impl SdaPin<T>> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::GlobalInterrupt, GlobalInterruptHandler<T>> + 'd,
        #[cfg(dma)] tx_dma: impl Peripheral<P = impl TxDma<T>> + 'd,
        #[cfg(dma)] rx_dma: impl Peripheral<P = impl RxDma<T>> + 'd,
        freq: Hertz,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(scl, config.scl_af()),
            new_pin!(sda, config.sda_af()),
            #[cfg(dma)] new_dma!(tx_dma),
            #[cfg(dma)] new_dma!(rx_dma),
            freq,
            config,
        )
    }

    pub fn into_slave_multimaster(
        mut self, 
        slave_addr_config: SlaveAddrConfig
    ) -> I2c<'d, Async, MultiMaster> {
        let mut slave = I2c {
            info: self.info,
            state: self.state,
            kernel_clock: self.kernel_clock,
            _drop_guard: self._drop_guard,
            #[cfg(dma)]
            tx_dma: self.tx_dma.take(),
            #[cfg(dma)]
            rx_dma: self.rx_dma.take(),
            #[cfg(feature = "time")]
            timeout: self.timeout,
            _phantom: PhantomData,
            _phantom2: PhantomData
        };
        slave.configure_slave(slave_addr_config);
        return slave;
    }
}

impl <'d, M: Mode> I2c<'d, M, MultiMaster> {
    fn configure_slave(
        &mut self,
        slave_addr_config: SlaveAddrConfig
    ) {
        self.info.regs.cr1().modify(|reg| {
            reg.set_pe(false);
        });
        self.info.regs.cr1().modify(|reg| {
            reg.set_nostretch(false);
            reg.set_engc(slave_addr_config.gencall);
            reg.set_pe(true);
        });
        self.info.regs.oar1().write(|reg| // py32 only has 7-bit addressing
            reg.set_add(slave_addr_config.own_addr)
        ); 
    }
    pub fn reconfigure_addresses(&mut self, slave_addr_config: SlaveAddrConfig) {
        self.info.regs.cr1().modify(|reg| {
            reg.set_engc(slave_addr_config.gencall);
        });
        self.info.regs.oar1().write(|reg| // py32 only has 7-bit addressing
            reg.set_add(slave_addr_config.own_addr)
        ); 
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Command {
    Read,
    GeneralCall(usize),
    Write(usize)
}

/// Possible responses to responding to a read
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadStatus {
    /// Transaction Complete, controller naked our last byte
    Done,
    /// Transaction Incomplete, controller trying to read more bytes than were provided
    NeedMoreBytes,
    /// Transaction Complete, but controller stopped reading bytes before we ran out
    LeftoverBytes(usize),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SlaveError {
    Abort(Error),
    InvalidResponseBufferLength,
    PartialWrite(usize),
    PartialGeneralCall(usize),
}


impl<'d> I2c<'d, Blocking, Master> {
    /// Create a new blocking I2C driver.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        scl: impl Peripheral<P = impl SclPin<T>> + 'd,
        sda: impl Peripheral<P = impl SdaPin<T>> + 'd,
        freq: Hertz,
        config: Config,
    ) -> Self {
        Self::new_inner(
            peri,
            new_pin!(scl, config.scl_af()),
            new_pin!(sda, config.sda_af()),
            #[cfg(dma)] None,
            #[cfg(dma)] None,
            freq,
            config,
        )
    }
}

impl<'d, B:Mode, M: MasterMode> I2c<'d, B, M> {
    /// Create a new I2C driver.
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        scl: Option<PeripheralRef<'d, AnyPin>>,
        sda: Option<PeripheralRef<'d, AnyPin>>,
        #[cfg(dma)] tx_dma: Option<ChannelAndRequest<'d>>,
        #[cfg(dma)] rx_dma: Option<ChannelAndRequest<'d>>,
        freq: Hertz,
        config: Config,
    ) -> Self {
        unsafe { T::GlobalInterrupt::enable() };

        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            #[cfg(dma)] tx_dma,
            #[cfg(dma)] rx_dma,
            #[cfg(feature = "time")]
            timeout: config.timeout,
            _drop_guard: I2CDropGuard {info: T::info(), scl: scl, sda: sda},
            _phantom: PhantomData,
            _phantom2: PhantomData
        };
        this.enable_and_init(freq, config);
        this
    }

    fn enable_and_init(&mut self, freq: Hertz, config: Config) {
        self.info.rcc.enable_and_reset();
        self.init(freq, config);
    }

    fn timeout(&self) -> Timeout {
        Timeout {
            #[cfg(feature = "time")]
            deadline: Instant::now() + self.timeout,
        }
    }
}


#[derive(Copy, Clone)]
struct Timeout {
    #[cfg(feature = "time")]
    deadline: Instant,
}

#[allow(dead_code)]
impl Timeout {
    #[inline]
    fn check(self) -> Result<(), Error> {
        #[cfg(feature = "time")]
        if Instant::now() > self.deadline {
            return Err(Error::Timeout);
        }

        Ok(())
    }

    #[inline]
    fn with<R>(
        self,
        fut: impl Future<Output = Result<R, Error>>,
    ) -> impl Future<Output = Result<R, Error>> {
        #[cfg(feature = "time")]
        {
            use futures_util::FutureExt;

            embassy_futures::select::select(embassy_time::Timer::at(self.deadline), fut).map(|r| {
                match r {
                    embassy_futures::select::Either::First(_) => Err(Error::Timeout),
                    embassy_futures::select::Either::Second(r) => r,
                }
            })
        }

        #[cfg(not(feature = "time"))]
        fut
    }
}

struct State {
    #[allow(unused)]
    waker: AtomicWaker,
}

impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }
}

struct Info {
    regs: crate::pac::i2c::I2c,
    rcc: RccInfo,
}

peri_trait!(
    irqs: [GlobalInterrupt],
);

pin_trait!(SclPin, Instance);
pin_trait!(SdaPin, Instance);
#[cfg(dma)] dma_trait!(RxDma, Instance);
#[cfg(dma)] dma_trait!(TxDma, Instance);

/// Global interrupt handler.
pub struct GlobalInterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::GlobalInterrupt> for GlobalInterruptHandler<T> {
    unsafe fn on_interrupt() {
        v1::on_interrupt::<T>()
    }
}

foreach_peripheral!(
    (i2c, $inst:ident) => {
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

        impl Instance for peripherals::$inst {
            type GlobalInterrupt = crate::_generated::peripheral_interrupts::$inst::GLOBAL;
        }
    };
);

impl<'d, B:Mode, M: MasterMode> embedded_hal_02::blocking::i2c::Read for I2c<'d, B, M> {
    type Error = Error;

    fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(address, buffer)
    }
}

impl<'d, B:Mode, M: MasterMode> embedded_hal_02::blocking::i2c::Write for I2c<'d, B, M> {
    type Error = Error;

    fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(address, write)
    }
}

impl<'d, B:Mode, M: MasterMode> embedded_hal_02::blocking::i2c::WriteRead for I2c<'d, B, M> {
    type Error = Error;

    fn write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.blocking_write_read(address, write, read)
    }
}

impl embedded_hal_1::i2c::Error for Error {
    fn kind(&self) -> embedded_hal_1::i2c::ErrorKind {
        match *self {
            Self::Bus => embedded_hal_1::i2c::ErrorKind::Bus,
            Self::Arbitration => embedded_hal_1::i2c::ErrorKind::ArbitrationLoss,
            Self::Nack => embedded_hal_1::i2c::ErrorKind::NoAcknowledge(
                embedded_hal_1::i2c::NoAcknowledgeSource::Unknown,
            ),
            Self::Timeout => embedded_hal_1::i2c::ErrorKind::Other,
            Self::Crc => embedded_hal_1::i2c::ErrorKind::Other,
            Self::Overrun => embedded_hal_1::i2c::ErrorKind::Overrun,
            Self::ZeroLengthTransfer => embedded_hal_1::i2c::ErrorKind::Other,
            Self::Busy => embedded_hal_1::i2c::ErrorKind::Other
        }
    }
}

impl<'d, B:Mode, M: MasterMode> embedded_hal_1::i2c::ErrorType for I2c<'d, B, M> {
    type Error = Error;
}

impl<'d, B:Mode, M: MasterMode> embedded_hal_1::i2c::I2c for I2c<'d, B, M> {
    fn read(&mut self, address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(address, read)
    }

    fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(address, write)
    }

    fn write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.blocking_write_read(address, write, read)
    }

    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal_1::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.blocking_transaction(address, operations)
    }
}

#[cfg(dma)] 
impl<'d, M:MasterMode> embedded_hal_async::i2c::I2c for I2c<'d, Async, M> {
    async fn read(&mut self, address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
        self.read(address, read).await
    }

    async fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Self::Error> {
        self.write(address, write).await
    }

    async fn write_read(
        &mut self,
        address: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.write_read(address, write, read).await
    }

    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal_1::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.transaction(address, operations).await
    }
}

/// Frame type in I2C transaction.
///
/// This tells each method what kind of framing to use, to generate a (repeated) start condition (ST
/// or SR), and/or a stop condition (SP). For read operations, this also controls whether to send an
/// ACK or NACK after the last byte received.
///
/// For write operations, the following options are identical because they differ only in the (N)ACK
/// treatment relevant for read operations:
///
/// - `FirstFrame` and `FirstAndNextFrame`
/// - `NextFrame` and `LastFrameNoStop`
///
/// Abbreviations used below:
///
/// - `ST` = start condition
/// - `SR` = repeated start condition
/// - `SP` = stop condition
/// - `ACK`/`NACK` = last byte in read operation
#[derive(Copy, Clone)]
#[allow(dead_code)]
enum FrameOptions {
    /// `[ST/SR]+[NACK]+[SP]` First frame (of this type) in transaction and also last frame overall.
    FirstAndLastFrame,
    /// `[ST/SR]+[NACK]` First frame of this type in transaction, last frame in a read operation but
    /// not the last frame overall.
    FirstFrame,
    /// `[ST/SR]+[ACK]` First frame of this type in transaction, neither last frame overall nor last
    /// frame in a read operation.
    FirstAndNextFrame,
    /// `[ACK]` Middle frame in a read operation (neither first nor last).
    NextFrame,
    /// `[NACK]+[SP]` Last frame overall in this transaction but not the first frame.
    LastFrame,
    /// `[NACK]` Last frame in a read operation but not last frame overall in this transaction.
    LastFrameNoStop,
}

#[allow(dead_code)]
impl FrameOptions {
    /// Sends start or repeated start condition before transfer.
    fn send_start(self) -> bool {
        match self {
            Self::FirstAndLastFrame | Self::FirstFrame | Self::FirstAndNextFrame => true,
            Self::NextFrame | Self::LastFrame | Self::LastFrameNoStop => false,
        }
    }

    /// Sends stop condition after transfer.
    fn send_stop(self) -> bool {
        match self {
            Self::FirstAndLastFrame | Self::LastFrame => true,
            Self::FirstFrame
            | Self::FirstAndNextFrame
            | Self::NextFrame
            | Self::LastFrameNoStop => false,
        }
    }

    /// Sends NACK after last byte received, indicating end of read operation.
    fn send_nack(self) -> bool {
        match self {
            Self::FirstAndLastFrame
            | Self::FirstFrame
            | Self::LastFrame
            | Self::LastFrameNoStop => true,
            Self::FirstAndNextFrame | Self::NextFrame => false,
        }
    }
}

/// Iterates over operations in transaction.
///
/// Returns necessary frame options for each operation to uphold the [transaction contract] and have
/// the right start/stop/(N)ACK conditions on the wire.
///
/// [transaction contract]: embedded_hal_1::i2c::I2c::transaction
#[allow(dead_code)]
fn operation_frames<'a, 'b: 'a>(
    operations: &'a mut [embedded_hal_1::i2c::Operation<'b>],
) -> Result<
    impl IntoIterator<Item = (&'a mut embedded_hal_1::i2c::Operation<'b>, FrameOptions)>,
    Error,
> {
    use embedded_hal_1::i2c::Operation::{Read, Write};

    // Check empty read buffer before starting transaction. Otherwise, we would risk halting with an
    // error in the middle of the transaction.
    //
    // In principle, we could allow empty read frames within consecutive read operations, as long as
    // at least one byte remains in the final (merged) read operation, but that makes the logic more
    // complicated and error-prone.
    if operations.iter().any(|op| match op {
        Read(read) => read.is_empty(),
        Write(_) => false,
    }) {
        return Err(Error::Overrun);
    }

    let mut operations = operations.iter_mut().peekable();

    let mut next_first_frame = true;

    Ok(iter::from_fn(move || {
        let Some(op) = operations.next() else {
            return None;
        };

        // Is `op` first frame of its type?
        let first_frame = next_first_frame;
        let next_op = operations.peek();

        // Get appropriate frame options as combination of the following properties:
        //
        // - For each first operation of its type, generate a (repeated) start condition.
        // - For the last operation overall in the entire transaction, generate a stop condition.
        // - For read operations, check the next operation: if it is also a read operation, we merge
        //   these and send ACK for all bytes in the current operation; send NACK only for the final
        //   read operation's last byte (before write or end of entire transaction) to indicate last
        //   byte read and release the bus for transmission of the bus master's next byte (or stop).
        //
        // We check the third property unconditionally, i.e. even for write opeartions. This is okay
        // because the resulting frame options are identical for write operations.
        let frame = match (first_frame, next_op) {
            (true, None) => FrameOptions::FirstAndLastFrame,
            (true, Some(Read(_))) => FrameOptions::FirstAndNextFrame,
            (true, Some(Write(_))) => FrameOptions::FirstFrame,
            //
            (false, None) => FrameOptions::LastFrame,
            (false, Some(Read(_))) => FrameOptions::NextFrame,
            (false, Some(Write(_))) => FrameOptions::LastFrameNoStop,
        };

        // Pre-calculate if `next_op` is the first operation of its type. We do this here and not at
        // the beginning of the loop because we hand out `op` as iterator value and cannot access it
        // anymore in the next iteration.
        next_first_frame = match (&op, next_op) {
            (_, None) => false,
            (Read(_), Some(Write(_))) | (Write(_), Some(Read(_))) => true,
            (Read(_), Some(Read(_))) | (Write(_), Some(Write(_))) => false,
        };

        Some((op, frame))
    }))
}
