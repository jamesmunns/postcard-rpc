use embassy_stm32::{
    bind_interrupts,
    peripherals::{self, PA11, PA12, USB_OTG_FS},
    usb_otg::{self, Driver},
};
use embassy_usb::{
    driver::EndpointError,
    msos::{self, windows_version},
    Builder, UsbDevice,
};
use static_cell::StaticCell;

pub type OtgDriver = Driver<'static, peripherals::USB_OTG_FS>;

pub static USB_BUFS: StaticCell<UsbBuffers> = StaticCell::new();

bind_interrupts!(pub struct Irqs {
    OTG_FS => usb_otg::InterruptHandler<peripherals::USB_OTG_FS>;
});

const DEVICE_INTERFACE_GUIDS: &[&str] = &["{AFB9A6FB-30BA-44BC-9232-806CFC875321}"];

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
    pub msos_descriptor: [u8; 256],
    pub ep_out_buffer: [u8; 256],
}

impl UsbBuffers {
    pub(crate) const fn new() -> Self {
        Self {
            device_descriptor: [0u8; 256],
            config_descriptor: [0u8; 256],
            bos_descriptor: [0u8; 256],
            msos_descriptor: [0u8; 256],
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
    <OtgDriver as embassy_usb::driver::Driver<'static>>::EndpointIn,
    <OtgDriver as embassy_usb::driver::Driver<'static>>::EndpointOut,
) {
    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let bufs = USB_BUFS.init(UsbBuffers::new());

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
        &mut bufs.msos_descriptor, // no msos descriptors
        &mut bufs.control_buf,
    );

    // Add the Microsoft OS Descriptor (MSOS/MOD) descriptor.
    // We tell Windows that this entire device is compatible with the "WINUSB" feature,
    // which causes it to use the built-in WinUSB driver automatically, which in turn
    // can be used by libusb/rusb software without needing a custom driver or INF file.
    // In principle you might want to call msos_feature() just on a specific function,
    // if your device also has other functions that still use standard class drivers.
    builder.msos_descriptor(windows_version::WIN8_1, 0);
    builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
    builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
        "DeviceInterfaceGUIDs",
        msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
    ));

    // Add a vendor-specific function (class 0xFF), and corresponding interface,
    // that uses our custom handler.
    let mut function = builder.function(0xFF, 0, 0);
    let mut interface = function.interface();
    let mut alt = interface.alt_setting(0xFF, 0, 0, None);
    let ep_out = alt.endpoint_bulk_out(64);
    let ep_in = alt.endpoint_bulk_in(64);
    drop(function);

    // Build the builder.
    let usb = builder.build();

    (usb, ep_in, ep_out)
}

#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, OtgDriver>) {
    usb.run().await;
}
