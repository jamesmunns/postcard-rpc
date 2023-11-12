use embassy_stm32::peripherals::{PA11, PA12, USB_OTG_FS};
use embassy_stm32::usb_otg::Driver;
use embassy_stm32::{bind_interrupts, peripherals, usb_otg};
use embassy_usb::class::cdc_acm::CdcAcmClass;
use embassy_usb::Builder;
use embassy_usb::{class::cdc_acm::State, driver::EndpointError, UsbDevice};
use static_cell::StaticCell;

pub type OtgDriver = Driver<'static, peripherals::USB_OTG_FS>;

pub static USB_BUFS: StaticCell<UsbBuffers> = StaticCell::new();
pub static USB_STATE: StaticCell<State> = StaticCell::new();

bind_interrupts!(pub struct Irqs {
    OTG_FS => usb_otg::InterruptHandler<peripherals::USB_OTG_FS>;
});

#[derive(defmt::Format, Debug)]
pub(crate) struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

pub(crate) struct UsbBuffers {
    pub device_descriptor: [u8; 256],
    pub config_descriptor: [u8; 256],
    pub bos_descriptor: [u8; 256],
    pub control_buf: [u8; 64],
    pub ep_out_buffer: [u8; 256],
}

impl UsbBuffers {
    pub(crate) const fn new() -> Self {
        Self {
            device_descriptor: [0u8; 256],
            config_descriptor: [0u8; 256],
            bos_descriptor: [0u8; 256],
            control_buf: [0u8; 64],
            ep_out_buffer: [0u8; 256],
        }
    }
}

pub struct UsbResources {
    pub periph: USB_OTG_FS,
    pub dp: PA12,
    pub dm: PA11,
}

pub fn configure_usb(
    p: UsbResources,
) -> (
    UsbDevice<'static, OtgDriver>,
    CdcAcmClass<'static, OtgDriver>,
) {
    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let bufs = USB_BUFS.init(UsbBuffers::new());
    let state = USB_STATE.init(State::new());

    // Create the driver, from the HAL.
    let mut config = embassy_stm32::usb_otg::Config::default();
    config.vbus_detection = true;
    let driver = Driver::new_fs(p.periph, Irqs, p.dp, p.dm, &mut bufs.ep_out_buffer, config);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB-serial example");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    let mut builder = Builder::new(
        driver,
        config,
        &mut bufs.device_descriptor,
        &mut bufs.config_descriptor,
        &mut bufs.bos_descriptor,
        &mut [], // no msos descriptors
        &mut bufs.control_buf,
    );

    // Create classes on the builder.
    let class = CdcAcmClass::new(&mut builder, state, 64);

    // Build the builder.
    let usb = builder.build();

    (usb, class)
}

#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, OtgDriver>) {
    usb.run().await;
}
