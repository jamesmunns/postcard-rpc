// use defmt::{info, warn};
// use embassy_executor::Spawner;
// use embassy_stm32::{peripherals::USB_OTG_FS, usb_otg::Driver};
// use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
// use embassy_time::{Duration, Timer};
// use embassy_usb::driver::{Endpoint, EndpointError, EndpointIn, EndpointOut};
// use james_icd::{
//     sleep::{Sleep, SleepDone, SleepEndpoint},
//     wire_error::{FatalError, ERROR_KEY},
// };
// use postcard_rpc::{
//     headered::{self, extract_header_from_bytes},
//     Endpoint as _, WireHeader,
// };
// use static_cell::StaticCell;

// type EpOut = <Driver<'static, USB_OTG_FS> as embassy_usb::driver::Driver<'static>>::EndpointOut;
// type EpIn = <Driver<'static, USB_OTG_FS> as embassy_usb::driver::Driver<'static>>::EndpointIn;
// pub type Sender = &'static Mutex<ThreadModeRawMutex, SenderInner>;

// pub struct SenderInner {
//     ep_in: EpIn,
// }

// impl SenderInner {
//     pub async fn send_all(&mut self, out: &[u8]) {
//         if out.is_empty() {
//             return;
//         }
//         self.ep_in.wait_enabled().await;
//         // write in segments of 64. The last chunk may
//         // be 0 < len <= 64.
//         for ch in out.chunks(64) {
//             if self.ep_in.write(ch).await.is_err() {
//                 return;
//             }
//         }
//         // If the total we sent was a multiple of 64, send an
//         // empty message to "flush" the transaction
//         if (out.len() & (64 - 1)) == 0 {
//             let _ = self.ep_in.write(&[]).await;
//         }
//     }
// }

// pub static SENDER: StaticCell<Mutex<ThreadModeRawMutex, SenderInner>> = StaticCell::new();

// pub fn init_sender(ep_in: EpIn) -> Sender {
//     SENDER.init(Mutex::new(SenderInner { ep_in }))
// }

// #[embassy_executor::task]
// pub async fn rpc_dispatch(mut ep_out: EpOut, sender: Sender) {
//     let mut buf = [0u8; 256];
//     'connect: loop {
//         // Wait for connection
//         ep_out.wait_enabled().await;

//         // For each packet...
//         'packet: loop {
//             // Accumulate a whole frame
//             let mut window = buf.as_mut_slice();
//             'buffer: loop {
//                 if window.is_empty() {
//                     defmt::println!("Overflow!");
//                     loop {
//                         // Just drain until the end of the overflow frame
//                         match ep_out.read(&mut buf).await {
//                             Ok(n) if n < 64 => {
//                                 // sender.lock().await.ep_in.write(b":(").await.ok();
//                                 continue 'packet;
//                             }
//                             Ok(_) => {}
//                             Err(EndpointError::BufferOverflow) => panic!(),
//                             Err(EndpointError::Disabled) => continue 'connect,
//                         };
//                     }
//                 }

//                 let n = match ep_out.read(window).await {
//                     Ok(n) => n,
//                     Err(EndpointError::BufferOverflow) => panic!(),
//                     Err(EndpointError::Disabled) => continue 'connect,
//                 };

//                 let (_now, later) = window.split_at_mut(n);
//                 window = later;
//                 if n != 64 {
//                     break 'buffer;
//                 }
//             }

//             // We now have a full frame! Great!
//             let wlen = window.len();
//             let len = buf.len() - wlen;
//             let frame = &buf[..len];
//             defmt::println!("got frame: {=usize}", frame.len());

//             // If it's for us, process it
//             dispatch(frame, sender).await;

//             // sender.lock().await.ep_in.write(b":)").await.unwrap();
//         }
//     }
// }

// async fn dispatch(msg: &[u8], sender: Sender) {
//     let Ok((hdr, body)) = extract_header_from_bytes(msg) else {
//         defmt::error!("Bad dispatch");
//         return;
//     };
//     let spawner = Spawner::for_current_executor().await;
//     let res: Result<(), FatalError> = match hdr.key {
//         james_icd::sleep::SleepEndpoint::REQ_KEY => {
//             sleep_handler(&hdr, sender, spawner, body).map_err(Into::into)
//         }
//         _ => Err(FatalError::UnknownEndpoint),
//     };

//     if let Err(e) = res {
//         let mut scratch = [0u8; 256];
//         if let Ok(m) = headered::to_slice_keyed(hdr.seq_no, ERROR_KEY, &e, &mut scratch) {
//             sender.lock().await.send_all(m).await;
//         }
//     }
// }

// enum CommsError {
//     PoolFull(u32),
//     Postcard,
// }

// impl From<CommsError> for FatalError {
//     fn from(value: CommsError) -> Self {
//         match value {
//             CommsError::PoolFull(_) => FatalError::NotEnoughSenders,
//             CommsError::Postcard => FatalError::WireFailure,
//         }
//     }
// }

fn sleep_handler(
    hdr: &WireHeader,
    sender: Sender,
    spawner: Spawner,
    bytes: &[u8],
) -> Result<(), CommsError> {
    info!("dispatching sleep...");
    if let Ok(msg) = postcard::from_bytes::<Sleep>(bytes) {
        if spawner.spawn(sleep_task(hdr.seq_no, sender, msg)).is_ok() {
            Ok(())
        } else {
            Err(CommsError::PoolFull(hdr.seq_no))
        }
    } else {
        warn!("Out of senders!");
        Err(CommsError::Postcard)
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn sleep_task(seq_no: u32, c: Sender, s: Sleep) {
    info!("Sleep spawned");
    Timer::after(Duration::from_secs(s.seconds.into())).await;
    Timer::after(Duration::from_micros(s.micros.into())).await;
    info!("Sleep complete");
    let mut buf = [0u8; 256];
    let msg = SleepDone { slept_for: s };
    if let Ok(used) =
        postcard_rpc::headered::to_slice_keyed(seq_no, SleepEndpoint::RESP_KEY, &msg, &mut buf)
    {
        c.lock().await.send_all(used).await;
    }
}
