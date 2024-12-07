use super::*;

/// USB bus.
pub struct Bus<'d, T: Instance> {
    pub(super) phantom: PhantomData<&'d mut T>,
    pub(super) ep_confs: [EndPointConfig; EP_COUNT],
    pub(super) inited: bool,
}

impl<'d, T: Instance> driver::Bus for Bus<'d, T> {
    async fn poll(&mut self) -> Event {
        poll_fn(move |cx| {
            BUS_WAKER.register(cx.waker());

            let regs = T::regs();

            // TODO: implement VBUS detection.
            if !self.inited {
                self.inited = true;
                return Poll::Ready(Event::PowerDetected);
            }

            if IRQ_RESUME.load(Ordering::Acquire) {
                IRQ_RESUME.store(false, Ordering::Relaxed);
                return Poll::Ready(Event::Resume);
            }

            if IRQ_RESET.load(Ordering::Acquire) {
                IRQ_RESET.store(false, Ordering::Relaxed);

                regs.power().write(|w| w.set_suspend_mode(true));
                // for index in 1..EP_COUNT {
                //     regs.index().write(|w| w.set_index(index as _));
                //     regs.in_csr1().modify(|w| w.set_flush_fifo(true));
                // }

                trace!("RESET");

                for w in &EP_IN_WAKERS {
                    w.wake()
                }
                for w in &EP_OUT_WAKERS {
                    w.wake()
                }

                return Poll::Ready(Event::Reset);
            }

            if IRQ_SUSPEND.load(Ordering::Acquire) {
                IRQ_SUSPEND.store(false, Ordering::Relaxed);
                return Poll::Ready(Event::Suspend);
            }

            Poll::Pending
        })
        .await
    }

    fn endpoint_set_stalled(&mut self, ep_addr: EndpointAddress, stalled: bool) {
        // This can race, so do a retry loop.
        let reg = T::regs();
        let ep_index = ep_addr.index();
        if ep_index != 0 {
            reg.index().write(|w| w.set_index(ep_index as _));
        }
        match ep_addr.direction() {
            Direction::In => {
                if ep_index == 0 {
                    // usb_ep0_state = USB_EP0_STATE_STALL;

                    reg.ep0_csr().write(|w| {
                        w.set_send_stall(stalled);
                        if stalled { w.set_serviced_out_pkt_rdy(true); }
                    });

                    // while !reg.ep0_csr().read().sent_stall() {}
                }
                else {
                    reg.in_csr1().write(|w| {
                        w.set_send_stall(stalled);
                        if !stalled {
                            w.set_sent_stall(false);
                            w.set_clr_data_tog(true);
                        }
                    });
                    // while !reg.in_csr1().read().sent_stall() {}             
                }
                EP_IN_WAKERS[ep_addr.index()].wake();
            }
            Direction::Out => {
                if ep_index == 0 {
                    // usb_ep0_state = USB_EP0_STATE_STALL;

                    reg.ep0_csr().write(|w| {
                        w.set_send_stall(stalled);
                        if stalled { w.set_serviced_out_pkt_rdy(true); }
                    });
                    // while !reg.ep0_csr().read().sent_stall() {}
                }
                else {
                    reg.out_csr1().write(|w| {
                        w.set_send_stall(stalled);
                        if !stalled {
                            w.set_sent_stall(false);
                            w.set_clr_data_tog(true);
                        }
                    });
                    // while !reg.out_csr1().read().sent_stall() {}   
                }
                EP_IN_WAKERS[ep_addr.index()].wake();
                EP_OUT_WAKERS[ep_addr.index()].wake();
            }
        }
    }

    fn endpoint_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        let reg = T::regs();
        let ep_index = ep_addr.index();
        if ep_index != 0 {
            reg.index().write(|w| w.set_index(ep_index as _));
        }

        if ep_index == 0 {
            // TODO: py32 offiial CherryUsb port returns false directly for EP0
            reg.ep0_csr().read().send_stall()
        } else {
            match ep_addr.direction() {
                Direction::In => reg.in_csr1().read().send_stall(),
                Direction::Out => reg.out_csr1().read().send_stall(),
            }
        }
    }

    fn endpoint_set_enabled(&mut self, ep_addr: EndpointAddress, enabled: bool) {
        trace!("set_enabled {:x} {}", ep_addr, enabled);
        let ep_index = ep_addr.index();
        
        if enabled {
            T::regs().index().write(|w| w.set_index(ep_index as u8));
            match ep_addr.direction() {
                Direction::Out => {
                    if ep_index == 0 {
                        T::regs().int_in1e().modify(|w| 
                            w.set_ep0(true))
                    } else {
                        T::regs().int_out1e().modify(|w| 
                            w.set_epout(ep_index - 1, true)
                        );
                    }
                    
                    // T::regs().out_csr2().write(|w| {
                    //     w.set_auto_clear(true);
                    // });
    
                    T::regs().max_pkt_out().write(|w|
                        w.set_max_pkt_size(self.ep_confs[ep_index].out_max_fifo_size_btyes)
                    );
    
                    T::regs().out_csr1().write(|w| {
                        w.set_clr_data_tog(true);
                    });
    
                    //TODO: DMA
    
                    if self.ep_confs[ep_index].ep_type == EndpointType::Isochronous {
                        T::regs().out_csr2().write(|w| {
                            w.set_iso(true);
                        });
                    }
    
                    if T::regs().out_csr1().read().out_pkt_rdy() {
                        T::regs().out_csr1().modify(|w| 
                            w.set_flush_fifo(true)
                        );
                    }
                    
                    let flags = EP_OUT_ENABLED.load(Ordering::Acquire) | ep_index as u8;
                    EP_OUT_ENABLED.store(flags, Ordering::Release);
                    // Wake `Endpoint::wait_enabled()`
                    EP_OUT_WAKERS[ep_index].wake();
                }
                Direction::In => {
                    if ep_index == 0 {
                        T::regs().int_in1e().modify(|w| 
                            w.set_ep0(true))
                    } else {
                        T::regs().int_in1e().modify(|w| 
                            w.set_epin(ep_index - 1, true)
                        );
                    }
    
                    // T::regs().in_csr2().write(|w| {
                    //     w.set_auto_set(true);
                    // });
    
                    // TODO: DMA
    
                    T::regs().max_pkt_in().write(|w|
                        w.set_max_pkt_size(self.ep_confs[ep_index].in_max_fifo_size_btyes)
                    );
    
                    T::regs().in_csr1().write(|w| {
                        w.set_clr_data_tog(true);
                    });
    
                    if self.ep_confs[ep_index].ep_type == EndpointType::Isochronous {
                        T::regs().in_csr2().write(|w| {
                            w.set_iso(true);
                        });
                    }
                    T::regs().in_csr2().write(|w| w.set_mode(Mode::IN));
    
                    if T::regs().in_csr1().read().fifo_not_empty() {
                        T::regs().in_csr1().modify(|w|    
                            w.set_flush_fifo(true)
                        );
                    }

                    let flags = EP_IN_ENABLED.load(Ordering::Acquire) | ep_index as u8;
                    EP_IN_ENABLED.store(flags, Ordering::Release);
                    // Wake `Endpoint::wait_enabled()`
                    EP_IN_WAKERS[ep_index].wake();
                }
            }
        }
        else {
            // py32 offiial CherryUsb port does nothing when disable an endpoint
            match ep_addr.direction() {
                Direction::Out => {
                    let flags = EP_OUT_ENABLED.load(Ordering::Acquire) & !(ep_index as u8);
                    EP_OUT_ENABLED.store(flags, Ordering::Release);
                }
                Direction::In => {
                    let flags = EP_IN_ENABLED.load(Ordering::Acquire) & !(ep_index as u8);
                    EP_IN_ENABLED.store(flags, Ordering::Release);
                }
            }
        }
    }

    async fn enable(&mut self) {
        T::regs().int_usb().write(|w| {
            w.set_reset(true);
            w.set_suspend(true);
            w.set_resume(true);
        });
    }
    async fn disable(&mut self) {}

    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        Err(Unsupported)
    }
}