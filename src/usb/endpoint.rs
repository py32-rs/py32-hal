use super::*;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(super) struct EndpointData {
    pub(super) ep_conf: EndPointConfig, // only valid if used_in || used_out
    pub(super) used_in: bool,
    pub(super) used_out: bool,
}

/// USB endpoint.
pub struct Endpoint<'d, T: Instance, D> {
    pub(super) _phantom: PhantomData<(&'d mut T, D)>,
    pub(super) info: EndpointInfo,
}

// impl<'d, T: Instance, > driver::Endpoint for Endpoint<'d, T, In> {
impl<'d, T: Instance, D: Dir> driver::Endpoint for Endpoint<'d, T, D> {
    fn info(&self) -> &EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        // TODO: seems unachievable
        #[cfg(feature = "time")]
        embassy_time::block_for(embassy_time::Duration::from_millis(10));
        #[cfg(not(feature = "time"))]
        cortex_m::asm::delay(unsafe { crate::rcc::get_freqs() }.sys.to_hertz().unwrap().0 / 100);
    }
}

impl<'d, T: Instance> driver::EndpointOut for Endpoint<'d, T, Out> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, EndpointError> {
        trace!("READ WAITING, buf.len() = {}", buf.len());
        let index = self.info.addr.index();
        let regs = T::regs();

        let _ = poll_fn(|cx| {
            EP_OUT_WAKERS[index].register(cx.waker());
            regs.index().write(|w| w.set_index(index as _));
            let ready = regs.out_csr1().read().out_pkt_rdy();

            if ready {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;

        regs.index().write(|w| w.set_index(index as _));

        let read_count = regs.out_count().read().outcount();
        
        if read_count as usize > buf.len() {
            return Err(EndpointError::BufferOverflow);
        }

        buf.into_iter().for_each(|b|
            *b = regs.fifo(index).read().data()
        );
        trace!("READ OK, rx_len = {}", read_count);

        Ok(read_count as usize)
    }
}

impl<'d, T: Instance> driver::EndpointIn for Endpoint<'d, T, In> {
    async fn write(&mut self, buf: &[u8]) -> Result<(), EndpointError> {
        if buf.len() > self.info.max_packet_size as usize {
            return Err(EndpointError::BufferOverflow);
        }

        let index = self.info.addr.index();
        let regs = T::regs();

        trace!("WRITE WAITING");

        let _ = poll_fn(|cx| {
            EP_IN_WAKERS[index].register(cx.waker());
            regs.index().write(|w| w.set_index(index as _));
            let unready = regs.in_csr1().read().in_pkt_rdy();

            if unready {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;

        regs.index().write(|w| w.set_index(index as _));

        if buf.len() == 0 {
            regs.in_csr1().modify(|w| w.set_in_pkt_rdy(true));
        } else {
            buf.into_iter().for_each(|b|
                regs.fifo(index).write(|w| w.set_data(*b))
            );

            regs.in_csr1().modify(|w| w.set_in_pkt_rdy(true));
        }
        trace!("WRITE OK");

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(super) struct EndPointConfig {
    pub(super) ep_type: EndpointType,
    pub(super) max_fifo_size: u8,

}