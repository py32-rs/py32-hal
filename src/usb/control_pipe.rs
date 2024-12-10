use super::*;

/// USB control pipe.
pub struct ControlPipe<'d, T: Instance> {
    pub(super) _phantom: PhantomData<&'d mut T>,
    pub(super) max_packet_size: u16,
    pub(super) ep_in: Endpoint<'d, T, In>,
    pub(super) ep_out: Endpoint<'d, T, Out>,
}

impl<'d, T: Instance> driver::ControlPipe for ControlPipe<'d, T> {
    fn max_packet_size(&self) -> usize {
        usize::from(self.max_packet_size)
    }

    async fn setup(&mut self) -> [u8; 8] {
        let regs = T::regs();
        loop {
            trace!("SETUP read waiting");
            poll_fn(|cx| {
                EP_OUT_WAKERS[0].register(cx.waker());
                
                regs.index().write(|w| w.set_index(0));

                if regs.ep0_csr().read().out_pkt_rdy() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;

            if regs.ep0_count().read().count() != 8 {
                trace!("SETUP read failed: {:?}", regs.ep0_count().read().count());
                continue;
            }

            let mut buf = [0; 8];
            (&mut buf).into_iter().for_each(|b|
                *b = regs.fifo(0).read().data()
            );
            regs.ep0_csr().modify(|w| w.set_serviced_out_pkt_rdy(true));

            trace!("SETUP read ok");
            return buf;
        }
    }

    async fn data_out(&mut self, buf: &mut [u8], first: bool, last: bool) -> Result<usize, EndpointError> {
        trace!("control: data_out len={} first={} last={}", buf.len(), first, last);

        let regs = T::regs();

        let _ = poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());
            // STC uses same usb IP with py32 (mentor usb),
            // which said it is nessery to set index to 0
            regs.index().write(|w| w.set_index(0));
            let ready = regs.ep0_csr().read().out_pkt_rdy();
            if ready {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;

        regs.index().write(|w| w.set_index(0));

        let read_count = regs.ep0_count().read().count();
        if read_count as usize > buf.len() {
            return Err(EndpointError::BufferOverflow);
        }

        if read_count as u16 > self.ep_out.info.max_packet_size {
            return Err(EndpointError::BufferOverflow);
        }

        buf.into_iter().for_each(|b|
            *b = regs.fifo(0).read().data()
        );
        regs.ep0_csr().modify(|w| w.set_serviced_out_pkt_rdy(true));
        trace!("READ OK, rx_len = {}", read_count);

        Ok(read_count as usize)
    }

    async fn data_in(&mut self, data: &[u8], first: bool, last: bool) -> Result<(), EndpointError> {
        trace!("control: data_in len={} first={} last={}", data.len(), first, last);

        if data.len() > self.ep_in.info.max_packet_size as usize {
            return Err(EndpointError::BufferOverflow);
        }

        let regs = T::regs();

        trace!("WRITE WAITING");

        let _ = poll_fn(|cx| {
            EP_IN_WAKERS[0].register(cx.waker());
            regs.index().write(|w| w.set_index(0));

             // TODO: use fifo_not_empty?
            let unready = regs.ep0_csr().read().in_pkt_rdy();

            if unready {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;

        regs.index().write(|w| w.set_index(0));

        data.into_iter().for_each(|b|
            regs.fifo(0).write(|w| w.set_data(*b))
        );

        regs.ep0_csr().modify(|w| {
            w.set_in_pkt_rdy(true);
            if last { w.set_data_end(true); }
        });
        Ok(())
    }

    async fn accept(&mut self) {
        trace!("control: accept");
        
        let regs = T::regs();
        regs.index().write(|w| w.set_index(0));

        // zero length
        regs.ep0_csr().modify(|w| {
            w.set_in_pkt_rdy(true);
            // w.set_data_end(true);
        });

        cortex_m::asm::delay(10000);

        // Wait is needed, so that we don't set the address too soon, breaking the status stage.
        // (embassy-usb sets the address after accept() returns)
        poll_fn(|cx| {
            EP_IN_WAKERS[0].register(cx.waker());
            regs.index().write(|w| w.set_index(0));

            // A zero-length OUT data packet is used to indicate the end of a Control transfer. In normal operation, such packets should only 
            // be received after the entire length of the device request has been transferred (i.e. after the CPU has set DataEnd). If, however, the 
            // host sends a zero-length OUT data packet before the entire length of device request has been transferred, this signals the 
            // premature end of the transfer. In this case, the MUSBMHDRC will automatically flush any IN token loaded by CPU ready for the 
            // Data phase from the FIFO and set SetupEnd. 
            if regs.ep0_csr().read().setup_end() {
                regs.ep0_csr().write(|w| w.set_serviced_setup_end(false));
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;

        trace!("control: accept OK");
    }

    async fn reject(&mut self) {
        let regs = T::regs();
        trace!("control: reject");

        // Set IN+OUT to stall
        regs.index().write(|w| w.set_index(0));
        regs.ep0_csr().modify(|w| {
            w.set_send_stall(true);
            w.set_serviced_out_pkt_rdy(true);
        });

        // TODO: async waiting for Sent Stall?
    }

    async fn accept_set_address(&mut self, addr: u8) {
        self.accept().await;

        let regs = T::regs();
        trace!("setting addr: {}", addr);
        regs.addr().write(|w| w.set_addr(addr));
    }
}