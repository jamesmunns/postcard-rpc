use core::fmt::Debug;
use pd_core::{headered::{to_slice, to_slice_keyed}, Dispatch, Key, WireHeader};
use postcard::experimental::schema::Schema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, Schema, Serialize, Deserialize)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Debug, Schema, Serialize, Deserialize)]
struct Pixel {
    position: u32,
    color: Rgb,
}

fn top<T: DeserializeOwned + Debug>(hdr: &WireHeader, ctxt: &mut usize, body: &[u8]) -> Result<(), DemoError> {
    *ctxt += 1;
    let msg = postcard::from_bytes::<T>(body).unwrap();
    println!("hash: {:?}; rqst: {ctxt}; seq: {}; Mapped to Top! {msg:?}", hdr.key, hdr.seq_no);
    Ok(())
}

fn bottom<T: DeserializeOwned + Debug>(hdr: &WireHeader, ctxt: &mut usize, body: &[u8]) -> Result<(), DemoError> {
    *ctxt += 1;
    let msg = postcard::from_bytes::<T>(body).unwrap();
    println!("hash: {:?}; rqst: {ctxt}; seq: {}; Mapped to Bottom! {msg:?}", hdr.key, hdr.seq_no);
    Ok(())
}

fn fav_color<T: DeserializeOwned + Debug>(hdr: &WireHeader, ctxt: &mut usize, body: &[u8]) -> Result<(), DemoError> {
    if *ctxt == 3 {
        return Err(DemoError::Lol);
    }
    *ctxt += 1;
    let msg = postcard::from_bytes::<T>(body).unwrap();
    println!("hash: {:?}; rqst: {ctxt}; seq: {}; Mapped to favorite color {msg:?}", hdr.key, hdr.seq_no);
    Ok(())
}

#[derive(Debug, PartialEq)]
enum DemoError {
    Lol,
}

fn main() {
    let mut map = Dispatch::<usize, DemoError, 16>::new(0);
    map.add_handler::<Pixel>("leds/top", top::<Pixel>).unwrap();
    map.add_handler::<Pixel>("leds/bottom", bottom::<Pixel>)
        .unwrap();
    map.add_handler::<Rgb>("favorite/color", fav_color::<Rgb>)
        .unwrap();

    let mut scratch = [0u8; 128];

    let msg = to_slice(
        10,
        "leds/top",
        &Pixel {
            position: 3,
            color: Rgb {
                r: 10,
                g: 20,
                b: 30,
            },
        },
        &mut scratch,
    )
    .unwrap();
    map.dispatch(msg).unwrap();

    let key = Key::for_path::<Pixel>("leds/bottom");

    let msg = to_slice_keyed(
        20,
        key,
        &Pixel {
            position: 3,
            color: Rgb {
                r: 10,
                g: 20,
                b: 30,
            },
        },
        &mut scratch,
    )
    .unwrap();
    map.dispatch(msg).unwrap();

    let msg = to_slice(
        30,
        "favorite/color",
        &Rgb {
            r: 10,
            g: 20,
            b: 30,
        },
        &mut scratch,
    )
    .unwrap();
    map.dispatch(msg).unwrap();

    let msg = to_slice(
        40,
        "favorite/color",
        &Rgb {
            r: 10,
            g: 20,
            b: 30,
        },
        &mut scratch,
    )
    .unwrap();
    println!("{:?}", map.dispatch(msg));
}
