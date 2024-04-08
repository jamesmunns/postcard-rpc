use std::collections::HashMap;

use nusb::{
    transfer::{Queue, RequestBuffer},
    DeviceInfo,
};
use postcard::experimental::schema::Schema;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::{error::TrySendError, Receiver, Sender};
use tokio::sync::Mutex;

use crate::{headered::extract_header_from_bytes, Key};

use super::{HostClient, HostContext, ProcessError, RpcFrame, SubInfo, WireContext};
use std::sync::Arc;

pub(crate) const BULK_OUT_EP: u8 = 0x01;
pub(crate) const BULK_IN_EP: u8 = 0x81;

fn raw_nusb_worker<F: FnMut(&DeviceInfo) -> bool>(func: F, ctx: WireContext) -> Result<(), ()> {
    let x = nusb::list_devices().unwrap().find(func).unwrap();
    let dev = x.open().unwrap();
    let interface = dev.claim_interface(0).unwrap();

    let boq = interface.bulk_out_queue(BULK_OUT_EP);
    let biq = interface.bulk_in_queue(BULK_IN_EP);

    let WireContext {
        outgoing,
        incoming,
        new_subs,
    } = ctx;

    let subs = Arc::new(Mutex::new(HashMap::new()));

    tokio::task::spawn(out_worker(boq, outgoing));
    tokio::task::spawn(in_worker(biq, incoming, subs.clone()));
    tokio::task::spawn(sub_worker(new_subs, subs));

    Ok(())
}

async fn out_worker(mut boq: Queue<Vec<u8>>, mut rec: Receiver<RpcFrame>) {
    loop {
        let Some(msg) = rec.recv().await else {
            panic!("TODO handle closing");
        };

        boq.submit(msg.to_bytes());

        let send_res = boq.next_complete().await;
        if send_res.status.is_err() {
            panic!("lol");
        }
    }
}

async fn in_worker(
    mut biq: Queue<RequestBuffer>,
    ctxt: Arc<HostContext>,
    subs: Arc<Mutex<HashMap<Key, Sender<RpcFrame>>>>,
) {
    for _ in 0..4 {
        biq.submit(RequestBuffer::new(1024));
    }

    loop {
        let res = biq.next_complete().await;

        if let Err(_e) = res.status {
            panic!("oh no");
        }

        // replace the submission
        biq.submit(RequestBuffer::new(1024));

        let Ok((hdr, body)) = extract_header_from_bytes(&res.data) else {
            println!("Decode error?");
            continue;
        };

        let mut handled = false;

        {
            let mut sg = subs.lock().await;
            let key = hdr.key;

            // Remove if sending fails
            let rem = if let Some(m) = sg.get(&key) {
                handled = true;
                let frame = RpcFrame {
                    header: hdr.clone(),
                    body: body.to_vec(),
                };
                let res = m.try_send(frame);

                match res {
                    Ok(()) => false,
                    Err(TrySendError::Full(_)) => {
                        println!("uh oh sub overflow");
                        false
                    }
                    Err(TrySendError::Closed(_)) => true,
                }
            } else {
                false
            };

            if rem {
                sg.remove(&key);
            }
        }

        if handled {
            continue;
        }

        let frame = RpcFrame {
            header: hdr,
            body: body.to_vec(),
        };
        if let Err(ProcessError::Closed) = ctxt.process(frame) {
            panic!();
        }
    }
}

async fn sub_worker(
    mut new_subs: Receiver<SubInfo>,
    subs: Arc<Mutex<HashMap<Key, Sender<RpcFrame>>>>,
) {
    while let Some(sub) = new_subs.recv().await {
        let mut sg = subs.lock().await;
        if let Some(_old) = sg.insert(sub.key, sub.tx) {
            // warn: replacing old ting
        }
    }
}

/// # Constructor Methods
///
/// These methods are used to create a new [HostClient] instance for use with tokio serial and cobs encoding.
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    pub fn new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
    ) -> Self {
        let (me, wire) = Self::new_manual(err_uri_path, outgoing_depth);

        raw_nusb_worker(func, wire).unwrap();

        me
    }
}
