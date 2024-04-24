use defmt::info;
use embassy_stm32::{peripherals::USB_OTG_FS, usb::Driver};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_time::{Duration, Timer};
use james_icd::sleep::{Sleep, SleepDone, SleepEndpoint};
use postcard_rpc::{define_dispatch, target_server::Sender, WireHeader};

define_dispatch!{
    dispatcher: Dispatcher<Mutex = ThreadModeRawMutex, Driver = Driver<'static, USB_OTG_FS>>;
    SleepEndpoint => spawn sleep_task,
}

#[embassy_executor::task(pool_size = 3)]
async fn sleep_task(header: WireHeader, s: Sleep, sender: Sender<ThreadModeRawMutex, Driver<'static, USB_OTG_FS>>) {
    info!("Sleep spawned");
    Timer::after(Duration::from_secs(s.seconds.into())).await;
    Timer::after(Duration::from_micros(s.micros.into())).await;
    info!("Sleep complete");
    let msg = SleepDone { slept_for: s };
    let _ = sender.reply::<SleepEndpoint>(header.seq_no, &msg).await;
}
