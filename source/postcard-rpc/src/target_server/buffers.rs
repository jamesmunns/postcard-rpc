pub struct AllBuffers<const EO: usize, const TX: usize, const RX: usize> {
    pub usb_device: UsbDeviceBuffers,
    pub endpoint_out: [u8; EO],
    pub tx_buf: [u8; TX],
    pub rx_buf: [u8; RX],
}

impl<const EO: usize, const TX: usize, const RX: usize> AllBuffers<EO, TX, RX> {
    pub const fn new() -> Self {
        Self {
            usb_device: UsbDeviceBuffers::new(),
            endpoint_out: [0u8; EO],
            tx_buf: [0u8; TX],
            rx_buf: [0u8; RX],
        }
    }
}

/// Buffers used by the [`UsbDevice`][embassy_usb::UsbDevice] of `embassy-usb`
pub struct UsbDeviceBuffers {
    pub config_descriptor: [u8; 256],
    pub bos_descriptor: [u8; 256],
    pub control_buf: [u8; 64],
    pub msos_descriptor: [u8; 256],
}

impl UsbDeviceBuffers {
    pub const fn new() -> Self {
        Self {
            config_descriptor: [0u8; 256],
            bos_descriptor: [0u8; 256],
            msos_descriptor: [0u8; 256],
            control_buf: [0u8; 64],
        }
    }
}
