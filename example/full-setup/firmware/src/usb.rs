use embassy_stm32::{peripherals, usb::Driver};
use embassy_usb::UsbDevice;


pub type OtgDriver = Driver<'static, peripherals::USB_OTG_FS>;

#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, OtgDriver>) {
    usb.run().await;
}
