use super::*;

/// USB driver.
pub struct Driver<'d, T: Instance> {
    phantom: PhantomData<&'d mut T>,
    alloc: [EndpointData; EP_COUNT],
}

impl<'d, T: Instance> Driver<'d, T> {
    /// Create a new USB driver.
    pub fn new(
        _usb: impl Peripheral<P = T> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        _dp: impl Peripheral<P = impl DpPin<T>> + 'd,
        _dm: impl Peripheral<P = impl DmPin<T>> + 'd,
    ) -> Self {
        let freq = T::frequency();
        if freq.0 != 48_000_000 {
            panic!("USB clock (PLL) must be 48MHz");
        }

        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
        rcc::enable_and_reset::<T>();

        let regs = T::regs();
        
        regs.index().write(|w| w.set_index(0));

        #[cfg(feature = "time")]
        embassy_time::block_for(embassy_time::Duration::from_millis(100));
        #[cfg(not(feature = "time"))]
        cortex_m::asm::delay(unsafe { crate::rcc::get_freqs() }.sys.to_hertz().unwrap().0 / 10);
         
        // Initialize the bus so that it signals that power is available
        BUS_WAKER.wake();

        Self {
            phantom: PhantomData,
            alloc: [EndpointData {
                ep_conf: EndPointConfig {
                    ep_type: EndpointType::Bulk,
                    in_max_fifo_size_btyes: 1,
                    out_max_fifo_size_btyes: 1,
                },
                used_in: false,
                used_out: false,
            }; EP_COUNT],
        }
    }

    fn alloc_endpoint<D: Dir>(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
        is_ep0: bool,
    ) -> Result<Endpoint<'d, T, D>, driver::EndpointAllocError> {
        trace!(
            "allocating type={:?} mps={:?} interval_ms={}, dir={:?}",
            ep_type,
            max_packet_size,
            interval_ms,
            D::dir()
        );


        let index = if is_ep0 {
            Some((0, &mut self.alloc[0]))
        }
        else {
            self.alloc.iter_mut().enumerate().find(|(i, ep)| {
                if *i == 0 {
                    return false; // reserved for control pipe
                }
                let used = ep.used_out || ep.used_in;
                
                #[cfg(all(not(feature = "allow-ep-shared-fifo"), py32f072))]
                if used { return false }

                #[cfg(py32f072)]
                if ((max_packet_size + 7) / 8) as u8 > MAX_FIFO_SIZE_BTYES[*i] {
                    return false;
                }

                #[cfg(py32f403)]
                if ((max_packet_size + 7) / 8) as u8 > MAX_FIFO_SIZE_BTYES {
                    panic!("max_packet_size > MAX_FIFO_SIZE");
                }

                let used_dir = match D::dir() {
                    Direction::Out => ep.used_out,
                    Direction::In => ep.used_in,
                };
                !used || (ep.ep_conf.ep_type == ep_type && !used_dir)
            })
        };

        let (index, ep) = match index {
            Some(x) => x,
            None => return Err(EndpointAllocError),
        };

        ep.ep_conf.ep_type = ep_type;
        

        T::regs().index().write(|w| w.set_index(index as u8));
        match D::dir() {
            Direction::Out => {
                assert!(!ep.used_out);
                ep.used_out = true;

                ep.ep_conf.out_max_fifo_size_btyes = calc_max_fifo_size_btyes(max_packet_size);
            }
            Direction::In => {
                assert!(!ep.used_in);
                ep.used_in = true;

                ep.ep_conf.in_max_fifo_size_btyes = calc_max_fifo_size_btyes(max_packet_size);
            }
        };

        Ok(Endpoint {
            _phantom: PhantomData,
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(index, D::dir()),
                ep_type,
                max_packet_size,
                interval_ms,
            },
        })
    }
}

impl<'d, T: Instance> driver::Driver<'d> for Driver<'d, T> {
    type EndpointOut = Endpoint<'d, T, Out>;
    type EndpointIn = Endpoint<'d, T, In>;
    type ControlPipe = ControlPipe<'d, T>;
    type Bus = Bus<'d, T>;

    fn alloc_endpoint_in(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointIn, driver::EndpointAllocError> {
        self.alloc_endpoint(ep_type, max_packet_size, interval_ms, false)
    }

    fn alloc_endpoint_out(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointOut, driver::EndpointAllocError> {
        self.alloc_endpoint(ep_type, max_packet_size, interval_ms, false)
    }

    fn start(mut self, control_max_packet_size: u16) -> (Self::Bus, Self::ControlPipe) {
        let ep_out = self
            .alloc_endpoint(EndpointType::Control, control_max_packet_size, 0, true)
            .unwrap();
        let ep_in = self
            .alloc_endpoint(EndpointType::Control, control_max_packet_size, 0, true)
            .unwrap();
        
        trace!("enabled");

        let mut ep_confs = [EndPointConfig {
            ep_type: EndpointType::Bulk,
            in_max_fifo_size_btyes: 1,
            out_max_fifo_size_btyes: 1,
        }; EP_COUNT];
        
        for i in 0..EP_COUNT {
            ep_confs[i] = self.alloc[i].ep_conf;
        }

        (
            Bus {
                phantom: PhantomData,
                ep_confs,
                inited: false,
            },
            ControlPipe {
                _phantom: PhantomData,
                max_packet_size: control_max_packet_size,
                ep_out,
                ep_in,
            },
        )
    }
}