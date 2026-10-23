//! # I2Cv1

// The following code is modified from embassy-stm32
// https://github.com/embassy-rs/embassy/tree/main/embassy-stm32
// Special thanks to the Embassy Project and its contributors for their work!

#[cfg(dma)]
use core::future::Future;
use core::future::poll_fn;
#[cfg(dma)]
use core::pin::Pin;
use core::task::Poll;

use embassy_embedded_hal::SetConfig;
#[cfg(dma)]
use embassy_futures::select::{Either, select};
use embassy_hal_internal::drop::OnDrop;
use embedded_hal_1::i2c::Operation;

use super::*;
use crate::mode::Mode as PeriMode;
use crate::pac::i2c;

// /!\                      /!\
// /!\ Implementation note! /!\
// /!\                      /!\
//
// It's somewhat unclear whether using interrupts here in a *strictly* one-shot style is actually
// what we want! If you are looking in this file because you are doing async I2C and your code is
// just totally hanging (sometimes), maybe swing by this issue:
// <https://github.com/embassy-rs/embassy/issues/2372>.
//
// There's some more details there, and we might have a fix for you. But please let us know if you
// hit a case like this!
pub unsafe fn on_interrupt<T: Instance>() {
    let regs = T::info().regs;
    // i2c v2 only woke the task on transfer complete interrupts. v1 uses interrupts for a bunch of
    // other stuff, so we wake the task on every interrupt.
    T::state().waker.wake();
    critical_section::with(|_| {
        // Clear event interrupt flag.
        regs.cr2().modify(|w| {
            w.set_itbufen(false);
            w.set_itevten(false);
            w.set_iterren(false);
        });
    });
}

impl<'d, M: PeriMode, IM: MasterMode> I2c<'d, M, IM> {
    pub(crate) fn init(&mut self, config: Config) {
        self.info.regs.cr1().modify(|reg| {
            reg.set_pe(false);
            //reg.set_anfoff(false);
        });

        // Errata: "Start cannot be generated after a misplaced Stop"
        //
        // > If a master generates a misplaced Stop on the bus (bus error)
        // > while the microcontroller I2C peripheral attempts to switch to
        // > Master mode by setting the START bit, the Start condition is
        // > not properly generated.
        //
        // This also can occur with falsely detected STOP events, for example
        // if the SDA line is shorted to low.
        //
        // The workaround for this is to trigger the SWRST line AFTER power is
        // enabled, AFTER PE is disabled and BEFORE making any other configuration.
        //
        // It COULD be possible to apply this workaround at runtime, instead of
        // only on initialization, however this would require detecting the timeout
        // or BUSY lockup condition, and re-configuring the peripheral after reset.
        //
        // This presents as an ~infinite hang on read or write, as the START condition
        // is never generated, meaning the start event is never generated.
        self.info.regs.cr1().modify(|reg| {
            reg.set_swrst(true);
        });
        self.info.regs.cr1().modify(|reg| {
            reg.set_swrst(false);
        });

        let timings = Timings::new(self.kernel_clock, config.frequency);

        self.info.regs.cr2().modify(|reg| {
            reg.set_freq(timings.freq);
        });
        self.info.regs.ccr().modify(|reg| {
            reg.set_f_s(timings.mode.f_s());
            reg.set_duty(timings.duty.duty());
            reg.set_ccr(timings.ccr);
        });
        self.info.regs.trise().modify(|reg| {
            reg.set_trise(timings.trise);
        });

        self.info.regs.cr1().modify(|reg| {
            reg.set_pe(true);
        });
    }

    fn check_and_clear_error_flags(info: &'static Info) -> Result<i2c::regs::Sr1, Error> {
        // Note that flags should only be cleared once they have been registered. If flags are
        // cleared otherwise, there may be an inherent race condition and flags may be missed.
        let sr1 = info.regs.sr1().read();

        if sr1.pecerr() {
            info.regs.sr1().write(|reg| {
                reg.0 = !0;
                reg.set_pecerr(false);
            });
            return Err(Error::Crc);
        }

        if sr1.ovr() {
            info.regs.sr1().write(|reg| {
                reg.0 = !0;
                reg.set_ovr(false);
            });
            return Err(Error::Overrun);
        }

        if sr1.af() {
            info.regs.sr1().write(|reg| {
                reg.0 = !0;
                reg.set_af(false);
            });
            return Err(Error::Nack);
        }

        if sr1.arlo() {
            info.regs.sr1().write(|reg| {
                reg.0 = !0;
                reg.set_arlo(false);
            });
            return Err(Error::Arbitration);
        }

        // The errata indicates that BERR may be incorrectly detected. It recommends ignoring and
        // clearing the BERR bit instead.
        if sr1.berr() {
            info.regs.sr1().write(|reg| {
                reg.0 = !0;
                reg.set_berr(false);
            });
        }

        Ok(sr1)
    }

    fn write_bytes(
        &mut self,
        addr: u8,
        bytes: &[u8],
        timeout: Timeout,
        frame: FrameOptions,
    ) -> Result<(), Error> {
        if frame.send_start() {
            // Send a START condition

            self.info.regs.cr1().modify(|reg| {
                reg.set_start(true);
            });

            // Wait until START condition was generated
            while !Self::check_and_clear_error_flags(self.info)?.start() {
                timeout.check()?;
            }

            // Check if we were the ones to generate START
            if self.info.regs.cr1().read().start() || !self.info.regs.sr2().read().msl() {
                return Err(Error::Arbitration);
            }

            // Set up current address we're trying to talk to
            self.info.regs.dr().write(|reg| reg.set_dr(addr << 1));

            // Wait until address was sent
            // Wait for the address to be acknowledged
            // Check for any I2C errors. If a NACK occurs, the ADDR bit will never be set.
            while !Self::check_and_clear_error_flags(self.info)?.addr() {
                timeout.check()?;
            }

            // Clear condition by reading SR2
            let _ = self.info.regs.sr2().read();
        }

        // Send bytes
        for c in bytes {
            self.send_byte(*c, timeout)?;
        }

        // Wait for the last byte to finish transmitting. This is essential even when not sending
        // STOP - if we're about to send a repeated START (for write_read), we must wait for the
        // last byte to finish transmitting, otherwise the repeated START will interrupt the
        // ongoing byte transmission and corrupt the transfer.
        while !Self::check_and_clear_error_flags(self.info)?.btf() {
            timeout.check()?;
        }

        if frame.send_stop() {
            // Send a STOP condition
            self.info.regs.cr1().modify(|reg| reg.set_stop(true));
        }

        // Fallthrough is success
        Ok(())
    }

    fn send_byte(&self, byte: u8, timeout: Timeout) -> Result<(), Error> {
        // Wait until we're ready for sending
        while {
            // Check for any I2C errors. If a NACK occurs, the ADDR bit will never be set.
            !Self::check_and_clear_error_flags(self.info)?.txe()
        } {
            timeout.check()?;
        }

        // Push out a byte of data
        self.info.regs.dr().write(|reg| reg.set_dr(byte));

        // Wait until byte is transferred
        while {
            // Check for any potential error conditions.
            !Self::check_and_clear_error_flags(self.info)?.btf()
        } {
            timeout.check()?;
        }

        Ok(())
    }

    fn recv_byte(&self, timeout: Timeout) -> Result<u8, Error> {
        while {
            // Check for any potential error conditions.
            Self::check_and_clear_error_flags(self.info)?;

            !self.info.regs.sr1().read().rxne()
        } {
            timeout.check()?;
        }

        let value = self.info.regs.dr().read().dr();
        Ok(value)
    }

    fn blocking_read_timeout(
        &mut self,
        addr: u8,
        buffer: &mut [u8],
        timeout: Timeout,
        frame: FrameOptions,
    ) -> Result<(), Error> {
        let Some((last, buffer)) = buffer.split_last_mut() else {
            return Err(Error::Overrun);
        };

        if frame.send_start() {
            // Send a START condition and set ACK bit
            self.info.regs.cr1().modify(|reg| {
                reg.set_start(true);
                reg.set_ack(true);
            });

            // Wait until START condition was generated
            while !Self::check_and_clear_error_flags(self.info)?.start() {
                timeout.check()?;
            }

            // Check if we were the ones to generate START
            if self.info.regs.cr1().read().start() || !self.info.regs.sr2().read().msl() {
                return Err(Error::Arbitration);
            }

            // Set up current address we're trying to talk to
            self.info.regs.dr().write(|reg| reg.set_dr((addr << 1) + 1));

            // Wait until address was sent
            // Wait for the address to be acknowledged
            while !Self::check_and_clear_error_flags(self.info)?.addr() {
                timeout.check()?;
            }

            // Clear condition by reading SR2
            let _ = self.info.regs.sr2().read();
        }

        // Receive bytes into buffer
        for c in buffer {
            *c = self.recv_byte(timeout)?;
        }

        // Prepare to send NACK then STOP after next byte
        self.info.regs.cr1().modify(|reg| {
            if frame.send_nack() {
                reg.set_ack(false);
            }
            if frame.send_stop() {
                reg.set_stop(true);
            }
        });

        // Receive last byte
        *last = self.recv_byte(timeout)?;

        // Fallthrough is success
        Ok(())
    }

    /// Blocking read.
    pub fn blocking_read(&mut self, addr: u8, read: &mut [u8]) -> Result<(), Error> {
        self.blocking_read_timeout(addr, read, self.timeout(), FrameOptions::FirstAndLastFrame)
    }

    /// Blocking write.
    pub fn blocking_write(&mut self, addr: u8, write: &[u8]) -> Result<(), Error> {
        self.write_bytes(addr, write, self.timeout(), FrameOptions::FirstAndLastFrame)?;

        // Fallthrough is success
        Ok(())
    }

    /// Blocking write, restart, read.
    pub fn blocking_write_read(
        &mut self,
        addr: u8,
        write: &[u8],
        read: &mut [u8],
    ) -> Result<(), Error> {
        // Check empty read buffer before starting transaction. Otherwise, we would not generate the
        // stop condition below.
        if read.is_empty() {
            return Err(Error::Overrun);
        }

        let timeout = self.timeout();

        self.write_bytes(addr, write, timeout, FrameOptions::FirstFrame)?;
        self.blocking_read_timeout(addr, read, timeout, FrameOptions::FirstAndLastFrame)?;

        Ok(())
    }

    /// Blocking transaction with operations.
    ///
    /// Consecutive operations of same type are merged. See [transaction contract] for details.
    ///
    /// [transaction contract]: embedded_hal_1::i2c::I2c::transaction
    pub fn blocking_transaction(
        &mut self,
        addr: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        let timeout = self.timeout();

        for (op, frame) in operation_frames(operations)? {
            match op {
                Operation::Read(read) => self.blocking_read_timeout(addr, read, timeout, frame)?,
                Operation::Write(write) => self.write_bytes(addr, write, timeout, frame)?,
            }
        }

        Ok(())
    }

    // Async
    /// Enable the event and error interrupts of the peripheral.
    #[cfg(dma)]
    #[inline] // pretty sure this should always be inlined
    fn enable_interrupts(info: &'static Info) {
        // The interrupt handler disables interrupts globally, so we need to re-enable them
        // This must be done in a critical section to avoid races
        critical_section::with(|_| {
            info.regs.cr2().modify(|w| {
                w.set_iterren(true);
                w.set_itevten(true);
            });
        });
    }

    /// Enable the interrupts of the peripheral, including the buffer interrupts.
    #[inline]
    fn enable_buffer_interrupts(info: &'static Info) {
        // The interrupt handler disables interrupts globally, so we need to re-enable them
        // This must be done in a critical section to avoid races
        critical_section::with(|_| {
            info.regs.cr2().modify(|w| {
                w.set_itbufen(true);
                w.set_iterren(true);
                w.set_itevten(true);
            });
        });
    }

    /// Disable DMA and interrupts in a critical section
    fn disable_dma_and_interrupts(info: &'static Info) {
        critical_section::with(|_| {
            info.regs.cr2().modify(|w| {
                w.set_dmaen(false);
                w.set_itbufen(false);
                w.set_iterren(false);
                w.set_itevten(false);
            });
        });
    }

    /// Wait until `pred` is satisfied by `SR1`, or an I2C error occurs.
    async fn wait_sr1(&self, pred: fn(&i2c::regs::Sr1) -> bool) -> Result<i2c::regs::Sr1, Error> {
        poll_fn(|cx| {
            self.state.waker.register(cx.waker());

            match Self::check_and_clear_error_flags(self.info) {
                Err(e) => Poll::Ready(Err(e)),
                Ok(sr1) => {
                    if pred(&sr1) {
                        Poll::Ready(Ok(sr1))
                    } else {
                        // When pending, (re-)enable interrupts to wake us up.
                        Self::enable_buffer_interrupts(self.info);
                        Poll::Pending
                    }
                }
            }
        })
        .await
    }

    /// Check the conditions that terminate a slave transfer.
    fn check_termination(sr1: &i2c::regs::Sr1) -> Option<SlaveTermination> {
        if sr1.stopf() {
            Some(SlaveTermination::Stop)
        } else if sr1.addr() {
            Some(SlaveTermination::Restart)
        } else if sr1.af() {
            Some(SlaveTermination::Nack)
        } else {
            None
        }
    }

    /// Can be used by both blocking and async implementations
    fn clear_stop_flag(info: &'static Info) {
        trace!("I2C slave: clearing STOPF flag (v1 sequence)");
        // v1 requires: READ SR1 then WRITE CR1 to clear STOPF
        let _ = info.regs.sr1().read();
        info.regs.cr1().modify(|_| {}); // Dummy write to clear STOPF
    }
}

impl<'d, IM: MasterMode> I2c<'d, Async, IM> {
    /// Write a frame, using DMA when the driver owns the DMA channels.
    async fn write_frame(&mut self, address: u8, write_buffer: &[u8], frame: FrameOptions) -> Result<(), Error> {
        #[cfg(dma)]
        if self.tx_dma.is_some() && !write_buffer.is_empty() {
            return self.write_frame_dma(address, write_buffer, frame).await;
        }

        self.write_frame_irq(address, write_buffer, frame).await
    }

    /// Read a frame, using DMA when the driver owns the DMA channels.
    async fn read_frame(&mut self, address: u8, read_buffer: &mut [u8], frame: FrameOptions) -> Result<(), Error> {
        #[cfg(dma)]
        if self.rx_dma.is_some() && !read_buffer.is_empty() {
            return self.read_frame_dma(address, read_buffer, frame).await;
        }

        self.read_frame_irq(address, read_buffer, frame).await
    }

    /// Write a frame without DMA, driven by the peripheral interrupts.
    async fn write_frame_irq(&mut self, address: u8, write_buffer: &[u8], frame: FrameOptions) -> Result<(), Error> {
        let info = self.info;

        // Sentinel to disable the interrupts when an error occurs or the future is canceled.
        let on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        // Make sure the peripheral does not hand the data over to the DMA controller.
        self.info.regs.cr2().modify(|w| w.set_dmaen(false));

        if frame.send_start() {
            // Send a START condition
            self.info.regs.cr1().modify(|reg| {
                reg.set_start(true);
            });

            // Wait until START condition was generated
            self.wait_sr1(|sr1| sr1.start()).await?;

            // Check if we were the ones to generate START
            if self.info.regs.cr1().read().start() || !self.info.regs.sr2().read().msl() {
                return Err(Error::Arbitration);
            }

            // Set up current address we're trying to talk to
            self.info.regs.dr().write(|reg| reg.set_dr(address << 1));

            // Wait for the address to be acknowledged
            self.wait_sr1(|sr1| sr1.addr()).await?;

            // Clear condition by reading SR2
            self.info.regs.sr2().read();
        }

        // Send bytes
        for &byte in write_buffer {
            // Wait until we're ready for sending
            self.wait_sr1(|sr1| sr1.txe()).await?;

            // Push out a byte of data
            self.info.regs.dr().write(|reg| reg.set_dr(byte));

            // Wait until byte is transferred
            self.wait_sr1(|sr1| sr1.btf()).await?;
        }

        if frame.send_stop() {
            // Send a STOP condition
            self.info.regs.cr1().modify(|reg| reg.set_stop(true));
        }

        drop(on_drop);

        // Fallthrough is success
        Ok(())
    }

    /// Read a frame without DMA, driven by the peripheral interrupts.
    async fn read_frame_irq(&mut self, address: u8, read_buffer: &mut [u8], frame: FrameOptions) -> Result<(), Error> {
        if read_buffer.is_empty() {
            return Err(Error::Overrun);
        }

        // Some branches below depend on whether the buffer contains only a single byte.
        let single_byte = read_buffer.len() == 1;

        let info = self.info;

        // Sentinel to disable the interrupts when an error occurs or the future is canceled.
        let on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        // Make sure the peripheral does not hand the data over to the DMA controller.
        self.info.regs.cr2().modify(|w| w.set_dmaen(false));

        if frame.send_start() {
            // Send a START condition and set ACK bit
            self.info.regs.cr1().modify(|reg| {
                reg.set_start(true);
                reg.set_ack(true);
            });

            // Wait until START condition was generated
            self.wait_sr1(|sr1| sr1.start()).await?;

            // Check if we were the ones to generate START
            if self.info.regs.cr1().read().start() || !self.info.regs.sr2().read().msl() {
                return Err(Error::Arbitration);
            }

            // Set up current address we're trying to talk to
            self.info
                .regs
                .dr()
                .write(|reg| reg.set_dr((address << 1) + 1));

            // Wait for the address to be acknowledged
            self.wait_sr1(|sr1| sr1.addr()).await?;

            // 18.3.8: When a single byte must be received: the NACK must be programmed during EV6
            // event, i.e. program ACK=0 when ADDR=1, before clearing ADDR flag.
            if frame.send_nack() && single_byte {
                self.info.regs.cr1().modify(|w| {
                    w.set_ack(false);
                });
            }

            // Clear condition by reading SR2
            self.info.regs.sr2().read();
        } else {
            // Before starting reception of single byte (but without START condition, i.e. in case
            // of continued frame), program NACK to emit at end of this byte.
            if frame.send_nack() && single_byte {
                self.info.regs.cr1().modify(|w| {
                    w.set_ack(false);
                });
            }
        }

        // Receive all but the last byte. The last one is received after the NACK and the STOP
        // condition have been programmed.
        let (last, rest) = read_buffer.split_last_mut().expect("buffer is not empty");
        for byte in rest {
            *byte = self.recv_byte_irq().await?;
        }

        // Prepare to send NACK then STOP after next byte
        self.info.regs.cr1().modify(|reg| {
            if frame.send_nack() {
                reg.set_ack(false);
            }
            if frame.send_stop() {
                reg.set_stop(true);
            }
        });

        // Receive last byte
        *last = self.recv_byte_irq().await?;

        drop(on_drop);

        // Fallthrough is success
        Ok(())
    }

    /// Receive a single byte, driven by the peripheral interrupts.
    async fn recv_byte_irq(&mut self) -> Result<u8, Error> {
        self.wait_sr1(|sr1| sr1.rxne()).await?;

        Ok(self.info.regs.dr().read().dr())
    }

    #[cfg(dma)]
    async fn write_frame_dma(&mut self, address: u8, write: &[u8], frame: FrameOptions) -> Result<(), Error> {
        self.info.regs.cr2().modify(|w| {
            // Note: Do not enable the ITBUFEN bit in the I2C_CR2 register if DMA is used for
            // reception.
            w.set_itbufen(false);
            // DMA mode can be enabled for transmission by setting the DMAEN bit in the I2C_CR2
            // register.
            w.set_dmaen(true);
            // Sending NACK is not necessary (nor possible) for write transfer.
            w.set_last(false);
        });

        // Sentinel to disable transfer when an error occurs or future is canceled.
        // TODO: Generate STOP condition on cancel?
        let on_drop = OnDrop::new(|| {
            self.info.regs.cr2().modify(|w| {
                w.set_dmaen(false);
                w.set_iterren(false);
                w.set_itevten(false);
            })
        });

        if frame.send_start() {
            // Send a START condition
            self.info.regs.cr1().modify(|reg| {
                reg.set_start(true);
            });

            // Wait until START condition was generated
            poll_fn(|cx| {
                self.state.waker.register(cx.waker());

                match Self::check_and_clear_error_flags(self.info) {
                    Err(e) => Poll::Ready(Err(e)),
                    Ok(sr1) => {
                        if sr1.start() {
                            Poll::Ready(Ok(()))
                        } else {
                            // When pending, (re-)enable interrupts to wake us up.
                            Self::enable_interrupts(self.info);
                            Poll::Pending
                        }
                    }
                }
            })
            .await?;

            // Check if we were the ones to generate START
            if self.info.regs.cr1().read().start() || !self.info.regs.sr2().read().msl() {
                return Err(Error::Arbitration);
            }

            // Set up current address we're trying to talk to
            self.info.regs.dr().write(|reg| reg.set_dr(address << 1));

            // Wait for the address to be acknowledged
            poll_fn(|cx| {
                self.state.waker.register(cx.waker());

                match Self::check_and_clear_error_flags(self.info) {
                    Err(e) => Poll::Ready(Err(e)),
                    Ok(sr1) => {
                        if sr1.addr() {
                            Poll::Ready(Ok(()))
                        } else {
                            // When pending, (re-)enable interrupts to wake us up.
                            Self::enable_interrupts(self.info);
                            Poll::Pending
                        }
                    }
                }
            })
            .await?;

            // Clear condition by reading SR2
            self.info.regs.sr2().read();
        }

        let dma_transfer = unsafe {
            // Set the I2C_DR register address in the DMA_SxPAR register. The data will be moved to
            // this address from the memory after each TxE event.
            let dst = self.info.regs.dr().as_ptr() as *mut u8;

            self.tx_dma
                .as_mut()
                .unwrap()
                .write(write, dst, Default::default())
        };

        // Wait for bytes to be sent, or an error to occur.
        let poll_error = poll_fn(|cx| {
            self.state.waker.register(cx.waker());

            match Self::check_and_clear_error_flags(self.info) {
                Err(e) => Poll::Ready(Err::<(), Error>(e)),
                Ok(_) => {
                    // When pending, (re-)enable interrupts to wake us up.
                    Self::enable_interrupts(self.info);
                    Poll::Pending
                }
            }
        });

        // Wait for either the DMA transfer to successfully finish, or an I2C error to occur.
        match select(dma_transfer, poll_error).await {
            Either::Second(Err(e)) => Err(e),
            _ => Ok(()),
        }?;

        self.info.regs.cr2().modify(|w| {
            w.set_dmaen(false);
        });

        if frame.send_stop() {
            poll_fn(|cx| {
                self.state.waker.register(cx.waker());

                match Self::check_and_clear_error_flags(self.info) {
                    Err(e) => Poll::Ready(Err(e)),
                    Ok(sr1) => {
                        if sr1.btf() {
                            Poll::Ready(Ok(()))
                        } else {
                            // When pending, (re-)enable interrupts to wake us up.
                            Self::enable_interrupts(self.info);
                            Poll::Pending
                        }
                    }
                }
            })
            .await?;

            self.info.regs.cr1().modify(|w| {
                w.set_stop(true);
            });
        }

        drop(on_drop);

        // Fallthrough is success
        Ok(())
    }

    /// Write.
    pub async fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Error> {
        self.write_frame(address, write, FrameOptions::FirstAndLastFrame)
            .await?;

        Ok(())
    }

    /// Read.
    pub async fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Error> {
        self.read_frame(address, buffer, FrameOptions::FirstAndLastFrame)
            .await?;

        Ok(())
    }

    #[cfg(dma)]
    async fn read_frame_dma(&mut self, address: u8, read_buffer: &mut [u8], frame: FrameOptions) -> Result<(), Error> {
        if read_buffer.is_empty() {
            return Err(Error::Overrun);
        }

        // Some branches below depend on whether the buffer contains only a single byte.
        let single_byte = read_buffer.len() == 1;

        self.info.regs.cr2().modify(|w| {
            // Note: Do not enable the ITBUFEN bit in the I2C_CR2 register if DMA is used for
            // reception.
            w.set_itbufen(false);
            // DMA mode can be enabled for transmission by setting the DMAEN bit in the I2C_CR2
            // register.
            w.set_dmaen(true);
            // If, in the I2C_CR2 register, the LAST bit is set, I2C automatically sends a NACK
            // after the next byte following EOT_1. The user can generate a Stop condition in
            // the DMA Transfer Complete interrupt routine if enabled.
            w.set_last(frame.send_nack() && !single_byte);
        });

        // Sentinel to disable transfer when an error occurs or future is canceled.
        // TODO: Generate STOP condition on cancel?
        let on_drop = OnDrop::new(|| {
            self.info.regs.cr2().modify(|w| {
                w.set_dmaen(false);
                w.set_iterren(false);
                w.set_itevten(false);
            })
        });

        if frame.send_start() {
            // Send a START condition and set ACK bit
            self.info.regs.cr1().modify(|reg| {
                reg.set_start(true);
                reg.set_ack(true);
            });

            // Wait until START condition was generated
            poll_fn(|cx| {
                self.state.waker.register(cx.waker());

                match Self::check_and_clear_error_flags(self.info) {
                    Err(e) => Poll::Ready(Err(e)),
                    Ok(sr1) => {
                        if sr1.start() {
                            Poll::Ready(Ok(()))
                        } else {
                            // When pending, (re-)enable interrupts to wake us up.
                            Self::enable_interrupts(self.info);
                            Poll::Pending
                        }
                    }
                }
            })
            .await?;

            // Check if we were the ones to generate START
            if self.info.regs.cr1().read().start() || !self.info.regs.sr2().read().msl() {
                return Err(Error::Arbitration);
            }

            // Set up current address we're trying to talk to
            self.info
                .regs
                .dr()
                .write(|reg| reg.set_dr((address << 1) + 1));

            // Wait for the address to be acknowledged
            poll_fn(|cx| {
                self.state.waker.register(cx.waker());

                match Self::check_and_clear_error_flags(self.info) {
                    Err(e) => Poll::Ready(Err(e)),
                    Ok(sr1) => {
                        if sr1.addr() {
                            Poll::Ready(Ok(()))
                        } else {
                            // When pending, (re-)enable interrupts to wake us up.
                            Self::enable_interrupts(self.info);
                            Poll::Pending
                        }
                    }
                }
            })
            .await?;

            // 18.3.8: When a single byte must be received: the NACK must be programmed during EV6
            // event, i.e. program ACK=0 when ADDR=1, before clearing ADDR flag.
            if frame.send_nack() && single_byte {
                self.info.regs.cr1().modify(|w| {
                    w.set_ack(false);
                });
            }

            // Clear condition by reading SR2
            self.info.regs.sr2().read();
        } else {
            // Before starting reception of single byte (but without START condition, i.e. in case
            // of continued frame), program NACK to emit at end of this byte.
            if frame.send_nack() && single_byte {
                self.info.regs.cr1().modify(|w| {
                    w.set_ack(false);
                });
            }
        }

        // 18.3.8: When a single byte must be received: [snip] Then the user can program the STOP
        // condition either after clearing ADDR flag, or in the DMA Transfer Complete interrupt
        // routine.
        if frame.send_stop() && single_byte {
            self.info.regs.cr1().modify(|w| {
                w.set_stop(true);
            });
        }

        let dma_transfer = unsafe {
            // Set the I2C_DR register address in the DMA_SxPAR register. The data will be moved
            // from this address from the memory after each RxE event.
            let src = self.info.regs.dr().as_ptr() as *mut u8;

            self.rx_dma.as_mut().unwrap().read(src, read_buffer, Default::default())
        };

        // Wait for bytes to be received, or an error to occur.
        let poll_error = poll_fn(|cx| {
            self.state.waker.register(cx.waker());

            match Self::check_and_clear_error_flags(self.info) {
                Err(e) => Poll::Ready(Err::<(), Error>(e)),
                _ => {
                    // When pending, (re-)enable interrupts to wake us up.
                    Self::enable_interrupts(self.info);
                    Poll::Pending
                }
            }
        });

        match select(dma_transfer, poll_error).await {
            Either::Second(Err(e)) => Err(e),
            _ => Ok(()),
        }?;

        self.info.regs.cr2().modify(|w| {
            w.set_dmaen(false);
        });

        if frame.send_stop() && !single_byte {
            self.info.regs.cr1().modify(|w| {
                w.set_stop(true);
            });
        }

        drop(on_drop);

        // Fallthrough is success
        Ok(())
    }

    /// Write, restart, read.
    pub async fn write_read(
        &mut self,
        address: u8,
        write_buffer: &[u8],
        read_buffer: &mut [u8],
    ) -> Result<(), Error> {
        if read_buffer.is_empty() {
            return Err(Error::Overrun);
        }

        self.write_frame(address, write_buffer, FrameOptions::FirstFrame)
            .await?;
        self.read_frame(address, read_buffer, FrameOptions::FirstAndLastFrame)
            .await
    }

    /// Transaction with operations.
    ///
    /// Consecutive operations of same type are merged. See [transaction contract] for details.
    ///
    /// [transaction contract]: embedded_hal_1::i2c::I2c::transaction
    pub async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        for (op, frame) in operation_frames(operations)? {
            match op {
                Operation::Read(read_buffer) => self.read_frame(address, read_buffer, frame).await?,
                Operation::Write(write_buffer) => self.write_frame(address, write_buffer, frame).await?,
            }
        }

        Ok(())
    }
}

enum Mode {
    Fast,
    Standard,
}

impl Mode {
    fn f_s(&self) -> i2c::vals::FS {
        match self {
            Mode::Fast => i2c::vals::FS::FAST,
            Mode::Standard => i2c::vals::FS::STANDARD,
        }
    }
}

enum Duty {
    Duty2_1,
    Duty16_9,
}

impl Duty {
    fn duty(&self) -> i2c::vals::Duty {
        match self {
            Duty::Duty2_1 => i2c::vals::Duty::DUTY2_1,
            Duty::Duty16_9 => i2c::vals::Duty::DUTY16_9,
        }
    }
}

/// Result of attempting to send a byte in slave transmitter mode
#[derive(Debug, PartialEq)]
enum TransmitResult {
    /// Byte sent and ACKed by master - continue transmission
    Acknowledged,
    /// Byte sent but NACKed by master - normal end of read transaction
    NotAcknowledged,
    /// STOP condition detected - master terminated transaction
    Stopped,
    /// RESTART condition detected - master starting new transaction
    Restarted,
}

/// Result of attempting to receive a byte in slave receiver mode
#[derive(Debug, PartialEq)]
enum ReceiveResult {
    /// Data byte successfully received
    Data(u8),
    /// STOP condition detected - end of write transaction
    Stopped,
    /// RESTART condition detected - master starting new transaction
    Restarted,
}

/// Enumeration of slave transaction termination conditions
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum SlaveTermination {
    /// STOP condition received - normal end of transaction
    Stop,
    /// RESTART condition received - master starting new transaction
    Restart,
    /// NACK received - normal end of read transaction
    Nack,
}

/// Phase of an interrupt-driven slave transmit operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlaveTransmitPhase {
    /// Waiting for `TxE` to load the next byte.
    WaitTxE,
    /// Waiting for `BTF` to confirm that the loaded byte was clocked out.
    WaitBtf,
}

impl<'d, M: PeriMode> I2c<'d, M, Master> {
    /// Configure the I2C driver for slave operations, allowing for the driver to be used as a slave and a master (multimaster).
    ///
    /// The asynchronous slave transfers are moved by the DMA when the driver owns the DMA channels (see [`I2c::new`])
    /// and by the peripheral interrupts otherwise (see [`I2c::new_no_dma`]).
    pub fn into_slave_multimaster(self, slave_addr_config: SlaveAddrConfig) -> I2c<'d, M, MultiMaster> {
        let mut slave = I2c {
            info: self.info,
            state: self.state,
            kernel_clock: self.kernel_clock,
            #[cfg(dma)]
            tx_dma: self.tx_dma, // Move ownership
            #[cfg(dma)]
            rx_dma: self.rx_dma, // Move ownership
            #[cfg(feature = "time")]
            timeout: self.timeout,
            _marker: PhantomData,
            _marker2: PhantomData,
            _drop_guard: self._drop_guard, // Move the drop guard
        };
        slave.init_slave(slave_addr_config);
        slave
    }
}

// Address configuration methods
impl<'d, M: PeriMode, IM: MasterMode> I2c<'d, M, IM> {
    /// Initialize slave mode with address configuration
    pub(crate) fn init_slave(&mut self, config: SlaveAddrConfig) {
        trace!("I2C slave: initializing with config={:?}", config);

        // Disable peripheral for configuration
        self.info.regs.cr1().modify(|reg| reg.set_pe(false));

        // Configure slave addresses
        self.apply_address_configuration(config);

        // Enable the peripheral first
        self.info.regs.cr1().modify(|reg| reg.set_pe(true));

        // Enable peripheral with slave settings
        self.info.regs.cr1().modify(|reg| {
            reg.set_ack(true); // Enable acknowledgment for slave mode
            reg.set_nostretch(false); // Allow clock stretching for processing time
        });

        trace!("I2C slave: initialization complete");
    }

    /// Apply the complete address configuration for slave mode
    fn apply_address_configuration(&mut self, config: SlaveAddrConfig) {
        self.configure_primary_address(config.addr);

        // Configure general call detection
        if config.general_call {
            self.info.regs.cr1().modify(|w| w.set_engc(true));
        }
    }

    /// Configure the primary address (OA1) register
    fn configure_primary_address(&mut self, addr: u8) {
        self.info.regs.oar1().write(|reg| {
            reg.0 = ((addr & 0x7f) as u32) << 1;
        });
    }
}

impl<'d, M: PeriMode> I2c<'d, M, MultiMaster> {
    /// Configure DMA settings for slave operations (shared between read/write)
    #[cfg(dma)]
    fn setup_slave_dma_base(&mut self) {
        self.info.regs.cr2().modify(|w| {
            w.set_itbufen(false); // Always disable buffer interrupts when using DMA
            w.set_dmaen(true); // Enable DMA requests
            w.set_last(false); // LAST bit not used in slave mode for v1 hardware
        });
    }

    /// Listen for incoming I2C address match and return the command type
    ///
    /// This method blocks until the slave address is matched by a master.
    /// Returns the command type (Read/Write) and the matched address.
    pub fn blocking_listen(&mut self) -> Result<SlaveCommand, Error> {
        trace!("I2C slave: starting blocking listen for address match");
        let result = self.blocking_listen_with_timeout(self.timeout());
        trace!("I2C slave: blocking listen complete, result={:?}", result);
        result
    }

    /// Respond to a master read request by transmitting data
    ///
    /// Sends the provided data to the master. If the master requests more bytes
    /// than available, padding bytes (0x00) are sent until the master terminates
    /// the transaction with NACK.
    ///
    /// Returns the total number of bytes transmitted (including padding).
    pub fn blocking_respond_to_read(&mut self, data: &[u8]) -> Result<usize, Error> {
        trace!("I2C slave: starting blocking respond_to_read, data_len={}", data.len());
        let result = self.transmit_to_master(data, self.timeout());
        trace!("I2C slave: blocking respond_to_read complete, result={:?}", result);
        result
    }

    /// Respond to a master write request by receiving data
    ///
    /// Receives data from the master into the provided buffer. If the master
    /// sends more bytes than the buffer can hold, excess bytes are acknowledged
    /// but discarded.
    ///
    /// Returns the number of bytes stored in the buffer (not total received).
    pub fn blocking_respond_to_write(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        trace!("I2C slave: starting blocking respond_to_write, buffer_len={}", buffer.len());
        let result = self.receive_from_master(buffer, self.timeout());
        trace!("I2C slave: blocking respond_to_write complete, result={:?}", result);
        result
    }

    // Private implementation methods

    /// Wait for address match and determine transaction type
    fn blocking_listen_with_timeout(&mut self, timeout: Timeout) -> Result<SlaveCommand, Error> {
        // Ensure interrupts are disabled for blocking operation
        self.disable_i2c_interrupts();

        // Wait for address match (ADDR flag)
        loop {
            let sr1 = Self::read_status_and_handle_errors(self.info)?;

            if sr1.addr() {
                // Address matched - read SR2 to get direction and clear ADDR flag
                let sr2 = self.info.regs.sr2().read();
                let direction = if sr2.tra() {
                    SlaveCommandKind::Read
                } else {
                    SlaveCommandKind::Write
                };

                if sr1.stopf() && sr2.busy() {
                    // The STOP belongs to the previous transaction.
                    Self::clear_stop_flag(self.info);
                }

                // Use the static method instead of the instance method
                let matched_address = Self::decode_matched_address(sr2, self.info)?;
                trace!(
                    "I2C slave: address matched, direction={:?}, addr={:?}",
                    direction, matched_address
                );

                // Drop a byte left in DR by an earlier transaction
                Self::discard_stale_byte(self.info);

                return Ok(SlaveCommand {
                    kind: direction,
                    address: matched_address,
                });
            }

            // No address match yet, consume received byte
            if sr1.rxne() {
                let _ = self.info.regs.dr().read();
            } else if sr1.stopf() {
                Self::clear_stop_flag(self.info);
            }

            timeout.check()?;
        }
    }

    /// Transmit data to master in response to read request
    fn transmit_to_master(&mut self, data: &[u8], timeout: Timeout) -> Result<usize, Error> {
        let mut bytes_transmitted = 0;
        let mut padding_count = 0;

        loop {
            let byte_to_send = if bytes_transmitted < data.len() {
                data[bytes_transmitted]
            } else {
                padding_count += 1;
                0x00 // Send padding bytes when data is exhausted
            };

            match self.transmit_byte(byte_to_send, timeout) {
                Ok(TransmitResult::Acknowledged) => {
                    bytes_transmitted += 1;
                }
                Ok(TransmitResult::NotAcknowledged) => {
                    bytes_transmitted += 1; // Count the NACKed byte
                    break;
                }
                Ok(TransmitResult::Stopped | TransmitResult::Restarted) => {
                    break;
                }
                // The master ended the transaction before it asked for a byte.
                Err(Error::Bus) => {
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        if padding_count > 0 {
            trace!(
                "I2C slave: sent {} data bytes + {} padding bytes = {} total",
                data.len(),
                padding_count,
                bytes_transmitted
            );
        }

        Ok(bytes_transmitted)
    }

    /// Receive data from master during write request
    fn receive_from_master(&mut self, buffer: &mut [u8], timeout: Timeout) -> Result<usize, Error> {
        let mut bytes_stored = 0;

        // Receive bytes that fit in buffer
        while bytes_stored < buffer.len() {
            match self.receive_byte(timeout)? {
                ReceiveResult::Data(byte) => {
                    buffer[bytes_stored] = byte;
                    bytes_stored += 1;
                }
                ReceiveResult::Stopped | ReceiveResult::Restarted => {
                    return Ok(bytes_stored);
                }
            }
        }

        // Handle buffer overflow by discarding excess bytes
        if bytes_stored == buffer.len() {
            trace!("I2C slave: buffer full, discarding excess bytes");
            self.discard_excess_bytes(timeout)?;
        }

        Ok(bytes_stored)
    }

    /// Discard excess bytes when buffer is full
    fn discard_excess_bytes(&mut self, timeout: Timeout) -> Result<(), Error> {
        let mut discarded_count = 0;

        loop {
            match self.receive_byte(timeout)? {
                ReceiveResult::Data(_) => {
                    discarded_count += 1;
                    continue;
                }
                ReceiveResult::Stopped | ReceiveResult::Restarted => {
                    if discarded_count > 0 {
                        trace!("I2C slave: discarded {} excess bytes", discarded_count);
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    /// Send a single byte and wait for master's response
    fn transmit_byte(&mut self, byte: u8, timeout: Timeout) -> Result<TransmitResult, Error> {
        // Wait for transmit buffer ready
        self.wait_for_transmit_ready(timeout)?;

        // Send the byte
        self.info.regs.dr().write(|w| w.set_dr(byte));

        // Wait for transmission completion or master response
        self.wait_for_transmit_completion(timeout)
    }

    /// Wait until transmit buffer is ready (TXE flag set)
    fn wait_for_transmit_ready(&mut self, timeout: Timeout) -> Result<(), Error> {
        loop {
            let sr1 = self.info.regs.sr1().read();

            // Check for early termination conditions
            if let Some(result) = Self::check_early_termination(sr1) {
                return Err(self.handle_early_termination(result));
            }

            // Check for other error conditions
            self.check_for_hardware_errors(sr1)?;

            if sr1.txe() {
                return Ok(()); // Ready to transmit
            }

            timeout.check()?;
        }
    }

    /// Wait for byte transmission completion or master response
    fn wait_for_transmit_completion(&mut self, timeout: Timeout) -> Result<TransmitResult, Error> {
        loop {
            let sr1 = self.info.regs.sr1().read();

            // Check flags in priority order
            if sr1.af() {
                Self::clear_acknowledge_failure(self.info);
                return Ok(TransmitResult::NotAcknowledged);
            }

            if sr1.btf() {
                return Ok(TransmitResult::Acknowledged);
            }

            if sr1.stopf() {
                Self::clear_stop_flag(self.info);
                return Ok(TransmitResult::Stopped);
            }

            if sr1.addr() {
                return Ok(TransmitResult::Restarted);
            }

            // Check for other error conditions
            self.check_for_hardware_errors(sr1)?;

            timeout.check()?;
        }
    }

    /// Receive a single byte or detect transaction termination
    fn receive_byte(&mut self, timeout: Timeout) -> Result<ReceiveResult, Error> {
        loop {
            let sr1 = Self::read_status_and_handle_errors(self.info)?;

            // Check for received data first (prioritize data over control signals)
            if sr1.rxne() {
                let byte = self.info.regs.dr().read().dr();
                let _ = self.info.regs.sr1().read();
                return Ok(ReceiveResult::Data(byte));
            }

            // Check for transaction termination
            if sr1.addr() {
                return Ok(ReceiveResult::Restarted);
            }

            if sr1.stopf() {
                Self::clear_stop_flag(self.info);
                return Ok(ReceiveResult::Stopped);
            }

            timeout.check()?;
        }
    }

    /// Determine which slave address was matched based on SR2 flags
    fn decode_matched_address(sr2: i2c::regs::Sr2, info: &'static Info) -> Result<u8, Error> {
        if sr2.gencall() {
            Ok(0x00) // General call address
        } else {
            // OA1 (primary address) was matched
            Ok(((info.regs.oar1().read().0 >> 1) & 0x7f) as u8)
        }
    }

    // Helper methods for hardware interaction

    /// Read status register and handle I2C errors (except NACK in slave mode)
    fn read_status_and_handle_errors(info: &'static Info) -> Result<i2c::regs::Sr1, Error> {
        match Self::check_and_clear_error_flags(info) {
            Ok(sr1) => Ok(sr1),
            Err(Error::Nack) => {
                // In slave mode, NACK is normal protocol behavior, not an error
                Ok(info.regs.sr1().read())
            }
            Err(other_error) => Err(other_error),
        }
    }

    /// Check for conditions that cause early termination of operations
    fn check_early_termination(sr1: i2c::regs::Sr1) -> Option<TransmitResult> {
        match Self::check_termination(&sr1) {
            Some(SlaveTermination::Stop) => Some(TransmitResult::Stopped),
            Some(SlaveTermination::Restart) => Some(TransmitResult::Restarted),
            Some(SlaveTermination::Nack) => Some(TransmitResult::NotAcknowledged),
            None => None,
        }
    }

    /// Convert early termination to appropriate error
    fn handle_early_termination(&mut self, result: TransmitResult) -> Error {
        match result {
            TransmitResult::Stopped => {
                Self::clear_stop_flag(self.info);
                Error::Bus // Unexpected STOP during setup
            }
            TransmitResult::Restarted => {
                Error::Bus // Unexpected RESTART during setup
            }
            TransmitResult::NotAcknowledged => {
                Self::clear_acknowledge_failure(self.info);
                Error::Bus // Unexpected NACK during setup
            }
            TransmitResult::Acknowledged => {
                unreachable!() // This should never be passed to this function
            }
        }
    }

    /// Check for hardware-level I2C errors during transmission
    fn check_for_hardware_errors(&self, sr1: i2c::regs::Sr1) -> Result<(), Error> {
        if sr1.pecerr() || sr1.ovr() || sr1.arlo() || sr1.berr() {
            // Delegate to existing error handling
            Self::check_and_clear_error_flags(self.info)?;
        }
        Ok(())
    }

    /// Disable I2C event and error interrupts for blocking operations
    fn disable_i2c_interrupts(&mut self) {
        self.info.regs.cr2().modify(|w| {
            w.set_itevten(false);
            w.set_iterren(false);
        });
    }

    /// Clear the acknowledge failure flag
    fn clear_acknowledge_failure(info: &'static Info) {
        info.regs.sr1().write(|reg| {
            reg.0 = !0;
            reg.set_af(false);
        });
    }

    /// Discard a byte left in DR by an earlier transaction.
    fn discard_stale_byte(info: &'static Info) {
        if info.regs.sr1().read().rxne() {
            let _ = info.regs.dr().read();
        }
    }
}

impl<'d> I2c<'d, Async, MultiMaster> {
    /// Async listen for incoming I2C messages using interrupts
    ///
    /// Waits for a master to address this slave and returns the command type
    /// (Read/Write) and the matched address. This method will suspend until
    /// an address match occurs.
    pub async fn listen(&mut self) -> Result<SlaveCommand, Error> {
        trace!("I2C slave: starting async listen for address match");
        let state = self.state;
        let info = self.info;

        Self::enable_buffer_interrupts(info);

        let on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());

            loop {
                let sr1 = match Self::read_status_and_handle_errors(info) {
                    Ok(sr1) => sr1,
                    Err(e) => {
                        error!("I2C slave: error during listen: {:?}", e);
                        return Poll::Ready(Err(e));
                    }
                };

                if sr1.addr() {
                    let sr2 = info.regs.sr2().read();
                    let direction = if sr2.tra() {
                        SlaveCommandKind::Read
                    } else {
                        SlaveCommandKind::Write
                    };

                    if sr1.stopf() && sr2.busy() {
                        // The STOP belongs to the previous transaction.
                        Self::clear_stop_flag(info);
                    }

                    let matched_address = match Self::decode_matched_address(sr2, info) {
                        Ok(addr) => {
                            trace!(
                                "I2C slave: address matched, direction={:?}, addr={:?}",
                                direction, addr
                            );
                            addr
                        }
                        Err(e) => {
                            error!("I2C slave: failed to decode matched address: {:?}", e);
                            return Poll::Ready(Err(e));
                        }
                    };

                    Self::discard_stale_byte(info);

                    return Poll::Ready(Ok(SlaveCommand {
                        kind: direction,
                        address: matched_address,
                    }));
                }

                if sr1.stopf() {
                    Self::clear_stop_flag(info);
                    continue;
                }

                // No address match yet, consume a received byte
                if sr1.rxne() {
                    let _ = info.regs.dr().read();
                    continue;
                }

                // Wait for the next event
                Self::enable_buffer_interrupts(info);
                return Poll::Pending;
            }
        })
        .await;

        drop(on_drop);
        trace!("I2C slave: listen complete, result={:?}", result);
        result
    }

    /// Async respond to write command
    ///
    /// Receives data from the master into the provided buffer. If the master
    /// sends more bytes than the buffer can hold, excess bytes are acknowledged
    /// but discarded to prevent interrupt flooding.
    ///
    /// The data is moved by the RX DMA when the driver owns the DMA channels (see [`I2c::new`])
    /// and by the interrupts of the peripheral otherwise (see [`I2c::new_no_dma`]).
    ///
    /// Returns the number of bytes stored in the buffer (not total received).
    pub async fn respond_to_write(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        #[cfg(dma)]
        {
            // An empty buffer has nothing to hand to DMA, and the DMA transfer
            // length is limited to 16 bits. Both cases are handled by the CPU.
            if self.rx_dma.is_some() && !buffer.is_empty() && buffer.len() <= u16::MAX as usize {
                let state = self.state;
                let info = self.info;
                return self
                    .execute_slave_receive_transfer(buffer, state, info)
                    .await;
            }
        }

        self.respond_to_write_no_dma(buffer).await
    }

    /// Async respond to read command
    ///
    /// Transmits data to the master. If the master requests more bytes than
    /// available in the data buffer, padding bytes (0x00) are sent until the
    /// master terminates the transaction with NACK, STOP, or RESTART.
    ///
    /// The data is moved by the TX DMA when the driver owns the DMA channels (see [`I2c::new`])
    /// and by the interrupts of the peripheral otherwise (see [`I2c::new_no_dma`]).
    ///
    /// Returns the total number of bytes transmitted (data + padding).
    pub async fn respond_to_read(&mut self, data: &[u8]) -> Result<usize, Error> {
        #[cfg(dma)]
        {
            // Empty data has nothing to hand to DMA; padding is generated by
            // the CPU path. DMA transfers are limited to 16 bits of length.
            if self.tx_dma.is_some() && !data.is_empty() && data.len() <= u16::MAX as usize {
                let state = self.state;
                let info = self.info;
                return self
                    .execute_slave_transmit_transfer(data, state, info)
                    .await;
            }
        }

        self.respond_to_read_no_dma(data).await
    }

    /// Receive a master write transaction into `buffer`.
    ///
    /// Bytes beyond the buffer length are acknowledged and discarded. The
    /// future resolves when the master issues STOP or a repeated START.
    /// A repeated START leaves `ADDR` set so the next `listen()`
    /// can decode the new address phase.
    async fn respond_to_write_no_dma(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        trace!(
            "I2C slave: starting respond_to_write, buffer_len={}",
            buffer.len()
        );

        let state = self.state;
        let info = self.info;

        let on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        let mut stored = 0usize;

        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());

            loop {
                let sr1 = match Self::read_status_and_handle_errors(info) {
                    Ok(sr1) => sr1,
                    Err(e) => {
                        error!("I2C slave: error during receive transfer: {:?}", e);
                        return Poll::Ready(Err(e));
                    }
                };

                if sr1.rxne() {
                    let byte = info.regs.dr().read().dr();
                    if stored < buffer.len() {
                        buffer[stored] = byte;
                        stored += 1;
                    }

                    let _ = info.regs.sr1().read();
                    continue;
                }

                if sr1.stopf() {
                    Self::clear_stop_flag(info);
                    return Poll::Ready(Ok(stored));
                }

                if sr1.addr() {
                    return Poll::Ready(Ok(stored));
                }

                // Nothing to consume; wait for the next event.
                Self::enable_buffer_interrupts(info);
                return Poll::Pending;
            }
        })
        .await;

        drop(on_drop);
        trace!("I2C slave: respond_to_write complete, result={:?}", result);
        result
    }

    /// Async respond to read command
    ///
    /// Transmits data to the master. If the master requests more bytes
    /// than available in the data buffer, padding bytes (0x00) are sent until
    /// the master terminates the transaction with NACK, STOP, or RESTART.
    ///
    /// Returns the total number of bytes transmitted (data + padding).
    async fn respond_to_read_no_dma(&mut self, data: &[u8]) -> Result<usize, Error> {
        trace!(
            "I2C slave: starting respond_to_read, data_len={}",
            data.len()
        );

        let state = self.state;
        let info = self.info;

        let on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        let mut sent = 0usize;
        let mut phase = SlaveTransmitPhase::WaitTxE;

        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());

            loop {
                let sr1 = match Self::check_and_clear_error_flags(info) {
                    Err(Error::Nack) => {
                        if phase == SlaveTransmitPhase::WaitBtf {
                            sent += 1;
                        }
                        return Poll::Ready(Ok(sent));
                    }
                    Err(e) => {
                        error!("I2C slave: error during transmit transfer: {:?}", e);
                        return Poll::Ready(Err(e));
                    }
                    Ok(sr1) => sr1,
                };

                match phase {
                    SlaveTransmitPhase::WaitTxE => {
                        if sr1.stopf() {
                            Self::clear_stop_flag(info);
                            return Poll::Ready(Ok(sent));
                        }

                        if sr1.addr() {
                            return Poll::Ready(Ok(sent));
                        }

                        if sr1.txe() {
                            let byte = if sent < data.len() { data[sent] } else { 0x00 };
                            info.regs.dr().write(|reg| reg.set_dr(byte));
                            phase = SlaveTransmitPhase::WaitBtf;
                            continue;
                        }
                    }
                    SlaveTransmitPhase::WaitBtf => {
                        if sr1.btf() {
                            sent += 1;
                            phase = SlaveTransmitPhase::WaitTxE;
                            continue;
                        }

                        if sr1.stopf() {
                            Self::clear_stop_flag(info);
                            return Poll::Ready(Ok(sent));
                        }

                        if sr1.addr() {
                            return Poll::Ready(Ok(sent));
                        }
                    }
                }

                // Neither condition met, enable interrupts and wait
                Self::enable_buffer_interrupts(info);
                return Poll::Pending;
            }
        })
        .await;

        drop(on_drop);
        trace!("I2C slave: respond_to_read complete, result={:?}", result);
        result
    }

    // === Private Transfer Execution Methods ===

    /// Execute complete slave receive transfer with excess byte handling
    #[cfg(dma)]
    async fn execute_slave_receive_transfer(
        &mut self,
        buffer: &mut [u8],
        state: &'static State,
        info: &'static Info,
    ) -> Result<usize, Error> {
        let total_len = buffer.len();

        // Wait for the first data byte before the RX DMA is armed.
        let wait_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });
        let first = poll_fn(|cx| {
            state.waker.register(cx.waker());

            let sr1 = match Self::read_status_and_handle_errors(info) {
                Ok(sr1) => sr1,
                Err(e) => return Poll::Ready(Err(e)),
            };

            if sr1.rxne() {
                return Poll::Ready(Ok(true));
            }

            if sr1.stopf() || sr1.addr() {
                return Poll::Ready(Ok(false));
            }

            Self::enable_buffer_interrupts(info);
            Poll::Pending
        })
        .await;
        drop(wait_drop);

        if !first? {
            // Nothing for the DMA to do: a zero-length write, or a bare repeated START
            return self.respond_to_write_no_dma(buffer).await;
        }

        // DMA carries the data; buffer interrupts must stay disabled.
        self.setup_slave_dma_base();

        let _on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        let mut dma_transfer = unsafe {
            let src = info.regs.dr().as_ptr() as *mut u8;
            self.rx_dma
                .as_mut()
                .unwrap()
                .read(src, buffer, Default::default())
        };

        // Track whether transfer was terminated by I2C event (STOP/RESTART) vs DMA completion.
        // We only need to handle excess bytes if DMA completed (buffer full) and master
        // is still sending. If STOP/RESTART terminated the transfer, there are no excess bytes.
        let mut terminated_by_i2c_event = false;

        let mut pending_byte: Option<u8> = None;

        // Use poll_fn to monitor both DMA and I2C events, allowing us to get the
        // remaining DMA transfer count when STOP/RESTART is detected.
        // Returns Ok(remaining) where remaining is the DMA NDTR value when termination was detected.
        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());

            // Check for I2C errors first
            match Self::read_status_and_handle_errors(info) {
                Err(e) => {
                    error!("I2C slave: error during receive transfer: {:?}", e);
                    return Poll::Ready(Err(e));
                }
                Ok(sr1) => {
                    // Check for STOP or RESTART conditions
                    if let Some(termination) = Self::check_termination(&sr1) {
                        match termination {
                            SlaveTermination::Stop | SlaveTermination::Restart => {
                                // Freeze the DMA counter before measuring.
                                dma_transfer.request_stop();
                                while dma_transfer.is_running() {}

                                // Get DMA remaining count
                                let remaining = dma_transfer.get_remaining_transfers() as usize;

                                pending_byte = if info.regs.sr1().read().rxne() {
                                    Some(info.regs.dr().read().dr())
                                } else {
                                    None
                                };

                                // Clear the termination flag
                                match termination {
                                    SlaveTermination::Stop => Self::clear_stop_flag(info),
                                    SlaveTermination::Restart => {
                                        // ADDR flag will be handled by next listen() call
                                    }
                                    SlaveTermination::Nack => unreachable!(),
                                }

                                terminated_by_i2c_event = true;
                                trace!(
                                    "I2C slave: receive terminated by {:?}, received {} bytes (remaining {})",
                                    termination,
                                    total_len.saturating_sub(remaining),
                                    remaining
                                );
                                return Poll::Ready(Ok(remaining));
                            }
                            SlaveTermination::Nack => {
                                // Unexpected NACK during receive
                                return Poll::Ready(Err(Error::Bus));
                            }
                        }
                    }

                    if Pin::new(&mut dma_transfer).poll(cx).is_ready() {
                        trace!("I2C slave: DMA receive completed (buffer full)");
                        return Poll::Ready(Ok(0_usize)); // remaining = 0
                    }

                    // Neither condition met, enable interrupts and wait
                    Self::enable_interrupts(info);
                    Poll::Pending
                }
            }
        })
        .await;

        // Stop the DMA transfer - this releases the borrow on buffer
        drop(dma_transfer);
        Self::disable_dma_and_interrupts(info);

        // Calculate actual bytes received
        let result = match result {
            Ok(remaining) => {
                let mut received = total_len.saturating_sub(remaining);
                if let Some(byte) = pending_byte {
                    if received < total_len {
                        buffer[received] = byte;
                        received += 1;
                    }
                }
                trace!("I2C slave: receive complete, received {} bytes", received);
                Ok(received)
            }
            Err(e) => Err(e),
        };

        // Only handle excess bytes if DMA completed (buffer full) AND transfer was NOT
        // terminated by STOP/RESTART. If STOP/RESTART occurred, there are no more bytes coming.
        if let Ok(received) = result {
            if received == total_len && !terminated_by_i2c_event {
                self.handle_excess_bytes(state, info).await?;
            }
        }

        result
    }

    /// Execute complete slave transmit transfer with padding byte handling
    #[cfg(dma)]
    async fn execute_slave_transmit_transfer(
        &mut self,
        data: &[u8],
        state: &'static State,
        info: &'static Info,
    ) -> Result<usize, Error> {
        let total_len = data.len();

        self.setup_slave_dma_base();

        let _on_drop = OnDrop::new(|| {
            Self::disable_dma_and_interrupts(info);
        });

        let mut dma_transfer = unsafe {
            let dst = info.regs.dr().as_ptr() as *mut u8;
            self.tx_dma.as_mut().unwrap().write(data, dst, Default::default())
        };

        // Track whether transfer was terminated by I2C event (STOP/RESTART/NACK) vs DMA completion.
        // We only need to handle padding bytes if DMA completed and master wants more data.
        let mut terminated_by_i2c_event = false;

        // Helper to calculate actual bytes transmitted.
        // DMA pre-loads the next byte into DR before it's clocked out. When termination
        // occurs, if TXE is clear, there's a byte in DR that was loaded but never sent.
        let calc_bytes_sent = |remaining: usize, sr1: crate::pac::i2c::regs::Sr1| {
            let dma_sent = total_len.saturating_sub(remaining);
            // If TXE is clear, DR contains a byte that DMA loaded but master never clocked out
            if !sr1.txe() && dma_sent > 0 {
                dma_sent - 1
            } else {
                dma_sent
            }
        };

        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());

            let sr1 = info.regs.sr1().read();

            // Check for I2C errors first (NACK is expected for read termination)
            if sr1.af() {
                // NACK is normal - master doesn't want more data.
                Self::disable_dma_and_interrupts(info);
                dma_transfer.request_stop();
                while dma_transfer.is_running() {}

                info.regs.sr1().write(|reg| {
                    reg.0 = !0;
                    reg.set_af(false);
                });

                let remaining = dma_transfer.get_remaining_transfers() as usize;
                let sent = calc_bytes_sent(remaining, sr1);
                terminated_by_i2c_event = true;
                trace!(
                    "I2C slave: transmit terminated by Nack, sent {} bytes (remaining {}, txe={})",
                    sent,
                    remaining,
                    sr1.txe()
                );
                return Poll::Ready(Ok(sent));
            }

            // Check for the other error conditions
            match Self::check_and_clear_error_flags(info) {
                Err(e) => {
                    error!("I2C slave: error during transmit transfer: {:?}", e);
                    return Poll::Ready(Err(e));
                }
                Ok(_) => {}
            }

            // Check for STOP or RESTART conditions
            if let Some(termination) = Self::check_termination(&sr1) {
                match termination {
                    SlaveTermination::Stop | SlaveTermination::Restart => {
                        let remaining = dma_transfer.get_remaining_transfers() as usize;
                        let sent = calc_bytes_sent(remaining, sr1);

                        match termination {
                            SlaveTermination::Stop => Self::clear_stop_flag(info),
                            SlaveTermination::Restart => {
                                // ADDR flag will be handled by next listen() call
                            }
                            SlaveTermination::Nack => unreachable!(),
                        }

                        terminated_by_i2c_event = true;
                        trace!(
                            "I2C slave: transmit terminated by {:?}, sent {} bytes (remaining {}, txe={})",
                            termination,
                            sent,
                            remaining,
                            sr1.txe()
                        );
                        return Poll::Ready(Ok(sent));
                    }
                    SlaveTermination::Nack => {
                        // Handled above, the acknowledge failure is checked
                        // before the other conditions.
                        unreachable!()
                    }
                }
            }

            // Check if DMA transfer completed (all data sent)
            if !dma_transfer.is_running() {
                trace!("I2C slave: DMA transmit completed (all data sent)");
                return Poll::Ready(Ok(total_len));
            }

            // Register the DMA waker too: the transfer-complete interrupt
            // wakes the task when the whole buffer has been loaded.
            if Pin::new(&mut dma_transfer).poll(cx).is_ready() {
                return Poll::Ready(Ok(total_len));
            }

            // Neither condition met, enable interrupts and wait
            Self::enable_interrupts(info);
            Poll::Pending
        })
        .await;

        // Stop the DMA transfer
        drop(dma_transfer);
        Self::disable_dma_and_interrupts(info);

        // Only handle padding bytes if DMA completed (all data sent) AND transfer was NOT
        // terminated by STOP/RESTART/NACK. If terminated early, master doesn't want more data.
        if let Ok(sent) = result {
            if sent == total_len && !terminated_by_i2c_event {
                let padding_count = self.handle_padding_bytes(state, info).await?;
                let total_bytes = sent + padding_count;
                trace!(
                    "I2C slave: sent {} data bytes + {} padding bytes = {} total",
                    sent, padding_count, total_bytes
                );
                return Ok(total_bytes);
            }
        }

        result
    }

    /// Handle excess bytes after DMA buffer is full
    ///
    /// Reads and discards bytes until transaction termination to prevent interrupt flooding
    #[cfg(dma)]
    async fn handle_excess_bytes(&mut self, state: &'static State, info: &'static Info) -> Result<(), Error> {
        let mut discarded_count = 0;

        poll_fn(|cx| {
            state.waker.register(cx.waker());

            match Self::read_status_and_handle_errors(info) {
                Err(e) => {
                    error!("I2C slave: error while discarding excess bytes: {:?}", e);
                    Poll::Ready(Err(e))
                }
                Ok(sr1) => {
                    // Drain any pending data BEFORE checking for termination.
                    // This ensures we count all excess bytes even if STOP arrives
                    // at the same time as the last data byte.
                    if sr1.rxne() {
                        let _discarded_byte = info.regs.dr().read().dr();
                        discarded_count += 1;
                        Self::enable_buffer_interrupts(info);
                        return Poll::Pending;
                    }

                    if let Some(termination) = Self::check_termination(&sr1) {
                        match termination {
                            SlaveTermination::Stop => Self::clear_stop_flag(info),
                            SlaveTermination::Restart => {}
                            SlaveTermination::Nack => unreachable!("NACK not expected during receive"),
                        }
                        if discarded_count > 0 {
                            trace!("I2C slave: discarded {} excess bytes", discarded_count);
                        }
                        return Poll::Ready(Ok(()));
                    }

                    Self::enable_buffer_interrupts(info);
                    Poll::Pending
                }
            }
        })
        .await
    }

    /// Handle padding bytes after DMA data is exhausted
    ///
    /// Sends 0x00 bytes until transaction termination to prevent interrupt flooding
    #[cfg(dma)]
    async fn handle_padding_bytes(&mut self, state: &'static State, info: &'static Info) -> Result<usize, Error> {
        let mut padding_count = 0;

        poll_fn(|cx| {
            state.waker.register(cx.waker());

            match Self::check_and_clear_error_flags(info) {
                Err(Error::Nack) => Poll::Ready(Ok(padding_count)),
                Err(e) => {
                    error!("I2C slave: error while sending padding bytes: {:?}", e);
                    Poll::Ready(Err(e))
                }
                Ok(sr1) => {
                    if let Some(termination) = Self::check_termination(&sr1) {
                        match termination {
                            SlaveTermination::Stop => Self::clear_stop_flag(info),
                            SlaveTermination::Restart => {}
                            SlaveTermination::Nack => {
                                info.regs.sr1().write(|reg| {
                                    reg.0 = !0;
                                    reg.set_af(false);
                                });
                            }
                        }
                        return Poll::Ready(Ok(padding_count));
                    }

                    if sr1.txe() {
                        info.regs.dr().write(|w| w.set_dr(0x00));
                        padding_count += 1;
                        Self::enable_buffer_interrupts(info);
                        return Poll::Pending;
                    }

                    Self::enable_buffer_interrupts(info);
                    Poll::Pending
                }
            }
        })
        .await
    }
}

/// Timing configuration for I2C v1 hardware
struct Timings {
    freq: u8,   // APB frequency in MHz for CR2.FREQ register
    mode: Mode, // Standard or Fast mode selection
    trise: u8,  // Rise time compensation value
    ccr: u16,   // Clock control register value
    duty: Duty, // Fast mode duty cycle selection
}

impl Timings {
    fn new(i2cclk: Hertz, frequency: Hertz) -> Self {
        // Calculate settings for I2C speed modes
        let frequency = frequency.0;
        let clock = i2cclk.0;
        let freq = clock / 1_000_000;
        assert!((2..=50).contains(&freq));

        // Configure bus frequency into I2C peripheral
        let trise = if frequency <= 100_000 {
            freq + 1
        } else {
            (freq * 300) / 1000 + 1
        };

        let mut ccr;
        let duty;
        let mode;

        // I2C clock control calculation
        if frequency <= 100_000 {
            duty = Duty::Duty2_1;
            mode = Mode::Standard;
            ccr = {
                let ccr = clock / (frequency * 2);
                if ccr < 4 { 4 } else { ccr }
            };
        } else {
            const DUTYCYCLE: u8 = 0;
            mode = Mode::Fast;
            if DUTYCYCLE == 0 {
                duty = Duty::Duty2_1;
                ccr = clock / (frequency * 3);
                ccr = if ccr < 1 { 1 } else { ccr };
            } else {
                duty = Duty::Duty16_9;
                ccr = clock / (frequency * 25);
                ccr = if ccr < 1 { 1 } else { ccr };
            }
        }

        Self {
            freq: freq as u8,
            trise: trise as u8,
            ccr: ccr as u16,
            duty,
            mode,
        }
    }
}

impl<'d, M: PeriMode, IM: MasterMode> SetConfig for I2c<'d, M, IM> {
    type Config = Hertz;
    type ConfigError = ();
    fn set_config(&mut self, config: &Self::Config) -> Result<(), ()> {
        let timings = Timings::new(self.kernel_clock, *config);
        self.info.regs.cr2().modify(|reg| {
            reg.set_freq(timings.freq);
        });
        self.info.regs.ccr().modify(|reg| {
            reg.set_f_s(timings.mode.f_s());
            reg.set_duty(timings.duty.duty());
            reg.set_ccr(timings.ccr);
        });
        self.info.regs.trise().modify(|reg| {
            reg.set_trise(timings.trise);
        });

        Ok(())
    }
}
