use core::fmt::Debug;
use pd_core::{headered::to_slice, Dispatch};
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

fn top<T: DeserializeOwned + Debug>(ctxt: &mut usize, body: &[u8]) -> Result<(), DemoError> {
    *ctxt += 1;
    let msg = postcard::from_bytes::<T>(body).unwrap();
    println!("rqst: {ctxt}; Mapped to Top! {msg:?}");
    Ok(())
}

fn bottom<T: DeserializeOwned + Debug>(ctxt: &mut usize, body: &[u8]) -> Result<(), DemoError> {
    *ctxt += 1;
    let msg = postcard::from_bytes::<T>(body).unwrap();
    println!("rqst: {ctxt}; Mapped to Bottom! {msg:?}");
    Ok(())
}

fn fav_color<T: DeserializeOwned + Debug>(ctxt: &mut usize, body: &[u8]) -> Result<(), DemoError> {
    if *ctxt == 3 {
        return Err(DemoError::Lol);
    }
    *ctxt += 1;
    let msg = postcard::from_bytes::<T>(body).unwrap();
    println!("rqst: {ctxt}; Mapped to favorite color {msg:?}");
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

    let msg = to_slice(
        "leds/bottom",
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
