// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

use core::marker::PhantomData;
use core::mem;
use core::sync::atomic::{Ordering, compiler_fence};

use embassy_hal_internal::Peri;
use py32_metapac::adc::vals::SampleTime;

use crate::adc::{Adc, AnyAdcChannel, Instance, RxDma, SealedAdcChannel};
use crate::dma::{Priority, ReadableRingBuffer, TransferOptions};
use crate::mode::Blocking;
use crate::rcc;

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct OverrunError;

fn clear_interrupt_flags(r: crate::pac::adc::Adc) {
    r.sr().modify(|regs| {
        regs.set_eoc(false);
        regs.set_ovr(false);
    });
}

pub struct RingBufferedAdc<'d, T: Instance> {
    _phantom: PhantomData<T>,
    ring_buf: ReadableRingBuffer<'d, u16>,
}

impl<'d, T: Instance> Adc<'d, T, Blocking> {
    /// Configures the ADC to use a DMA ring buffer for continuous data acquisition.
    ///
    /// The `dma_buf` should be large enough to prevent DMA buffer overrun.
    /// The length of the `dma_buf` should be a multiple of the ADC channel count.
    /// For example, if 3 channels are measured, its length can be 3 * 40 = 120 measurements.
    ///
    /// `read` method is used to read out measurements from the DMA ring buffer, and its buffer should be exactly half of the `dma_buf` length.
    /// It is critical to call `read` frequently to prevent DMA buffer overrun.
    ///
    /// [`read`]: #method.read
    pub fn into_ring_buffered(
        self,
        dma: Peri<'d, impl RxDma<T>>,
        dma_buf: &'d mut [u16],
        sequence: impl ExactSizeIterator<Item = (AnyAdcChannel<T>, SampleTime)>,
    ) -> RingBufferedAdc<'d, T> {
        assert!(!dma_buf.is_empty() && dma_buf.len() <= 0xFFFF);

        super::configure_sequence::<T>(
            sequence.map(|(channel, sample_time)| (channel.channel(), sample_time)),
        );

        let opts: crate::dma::TransferOptions = TransferOptions {
            half_transfer_ir: true,
            priority: Priority::VeryHigh,
            ..Default::default()
        };

        // Safety: we forget the struct before this function returns.
        let rx_src = T::regs().dr().as_ptr() as *mut u16;
        let request = dma.request();

        let ring_buf = unsafe { ReadableRingBuffer::new(dma, request, rx_src, dma_buf, opts) };

        // Don't disable the clock
        mem::forget(self);

        RingBufferedAdc {
            _phantom: PhantomData,
            ring_buf,
        }
    }
}

impl<'d, T: Instance> RingBufferedAdc<'d, T> {
    fn is_on() -> bool {
        T::regs().cr2().read().adon()
    }

    /// Turns on ADC if it is not already turned on and starts continuous DMA transfer.
    pub fn start(&mut self) -> Result<(), OverrunError> {
        self.setup_adc();
        self.ring_buf.clear();

        Ok(())
    }

    fn stop(&mut self, err: OverrunError) -> Result<usize, OverrunError> {
        self.teardown_adc();
        Err(err)
    }

    /// Stops DMA transfer.
    /// It does not turn off ADC.
    /// Calling `start` restarts continuous DMA transfer.
    ///
    /// [`start`]: #method.start
    pub fn teardown_adc(&mut self) {
        // Stop the DMA transfer
        self.ring_buf.request_pause();

        let r = T::regs();

        // Stop ADC
        r.cr2().modify(|reg| {
            // Stop ADC
            reg.set_swstart(false);
            // Stop DMA
            reg.set_dma(false);
        });

        r.cr1().modify(|w| {
            // Disable interrupt for end of conversion
            w.set_eocie(false);
            // Disable interrupt for overrun
            w.set_ovrie(false);
        });

        clear_interrupt_flags(r);

        compiler_fence(Ordering::SeqCst);
    }

    fn setup_adc(&mut self) {
        compiler_fence(Ordering::SeqCst);

        self.ring_buf.start();

        let r = T::regs();

        // Enable ADC
        let was_on = Self::is_on();
        if !was_on {
            r.cr2().modify(|reg| {
                reg.set_adon(false);
                reg.set_swstart(false);
            });
        }

        // Clear all interrupts
        r.sr().modify(|regs| {
            regs.set_eoc(false);
            regs.set_ovr(false);
            regs.set_strt(false);
        });

        r.cr1().modify(|w| {
            // Enable interrupt for end of conversion
            w.set_eocie(true);
            // Enable interrupt for overrun
            w.set_ovrie(true);
            // Scanning converisons of multiple channels
            w.set_scan(true);
            // Continuous conversion mode
            w.set_discen(false);
        });

        r.cr2().modify(|w| {
            // Enable DMA mode
            w.set_dma(true);
            // Enable continuous conversions
            w.set_cont(true);
        });

        // Begin ADC conversions
        T::regs().cr2().modify(|reg| {
            reg.set_adon(true);
            reg.set_swstart(true);
            reg.set_exttrig(true);
        });

        super::blocking_delay_us(3);
    }

    /// Read bytes that are readily available in the ring buffer.
    /// If no bytes are currently available in the buffer the call waits until the some
    /// bytes are available (at least one byte and at most half the buffer size)
    ///
    /// Background receive is started if `start()` has not been previously called.
    ///
    /// Receive in the background is terminated if an error is returned.
    /// It must then manually be started again by calling `start()` or by re-calling `read()`.
    pub fn blocking_read<const N: usize>(
        &mut self,
        buf: &mut [u16; N],
    ) -> Result<usize, OverrunError> {
        let r = T::regs();

        // Start background receive if it was not already started
        if !r.cr2().read().dma() {
            self.start()?;
        }

        // Clear overrun flag if set.
        if r.sr().read().ovr() {
            return self.stop(OverrunError);
        }

        loop {
            match self.ring_buf.read(buf) {
                Ok((0, _)) => {}
                Ok((len, _)) => {
                    return Ok(len);
                }
                Err(_) => {
                    return self.stop(OverrunError);
                }
            }
        }
    }

    /// Reads measurements from the DMA ring buffer.
    ///
    /// This method fills the provided `measurements` array with ADC readings from the DMA buffer.
    /// The length of the `measurements` array should be exactly half of the DMA buffer length. Because interrupts are only generated if half or full DMA transfer completes.
    ///
    /// Each call to `read` populates `measurements` in the conversion sequence
    /// order configured by [`Adc::into_ring_buffered`].
    ///
    /// If an error is returned, it indicates a DMA overrun, and the process must be restarted by calling `start` or `read` again.
    ///
    /// By default, the ADC fills the DMA buffer as quickly as possible. To control the sample rate, call `teardown_adc` after each readout, and then start the DMA again at the desired interval.
    /// Note that even if using `teardown_adc` to control the sample rate, with each call to `read`, measurements equivalent to half the size of the DMA buffer are still collected.
    ///
    /// Example:
    /// ```rust,ignore
    /// const DMA_BUF_LEN: usize = 120;
    /// let mut adc_dma_buf = [0u16; DMA_BUF_LEN];
    /// let sequence = [
    ///     (p.PA0.degrade_adc(), SampleTime::CYCLES239_5),
    ///     (p.PA1.degrade_adc(), SampleTime::CYCLES239_5),
    ///     (p.PA2.degrade_adc(), SampleTime::CYCLES239_5),
    /// ]
    /// .into_iter();
    /// let mut adc = adc.into_ring_buffered(p.DMA1_CH1, &mut adc_dma_buf, sequence);
    ///
    /// let mut measurements = [0u16; DMA_BUF_LEN / 2];
    /// loop {
    ///     match adc.read(&mut measurements).await {
    ///         Ok(_) => defmt::info!("adc1: {}", measurements),
    ///         Err(e) => defmt::warn!("Error: {:?}", e),
    ///     }
    /// }
    /// ```
    ///
    /// [`Adc::into_ring_buffered`]: Adc::into_ring_buffered
    /// [`teardown_adc`]: #method.teardown_adc
    /// [`start`]: #method.start
    pub async fn read<const N: usize>(
        &mut self,
        measurements: &mut [u16; N],
    ) -> Result<usize, OverrunError> {
        assert_eq!(
            self.ring_buf.capacity() / 2,
            N,
            "Buffer size must be half the size of the ring buffer"
        );

        let r = T::regs();

        // Start background receive if it was not already started
        if !r.cr2().read().dma() {
            self.start()?;
        }

        // Clear overrun flag if set.
        if r.sr().read().ovr() {
            return self.stop(OverrunError);
        }
        match self.ring_buf.read_exact(measurements).await {
            Ok(len) => Ok(len),
            Err(_) => self.stop(OverrunError),
        }
    }
}

impl<T: Instance> Drop for RingBufferedAdc<'_, T> {
    fn drop(&mut self) {
        self.teardown_adc();
        rcc::disable::<T>();
    }
}
