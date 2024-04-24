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

#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, OtgDriver>) {
    usb.run().await;
}
