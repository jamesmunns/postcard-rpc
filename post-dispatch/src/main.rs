use postcard::experimental::schema::Schema;
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use core::fmt::Debug;
use pd_core::{Dispatch, Key};

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

fn serialize_headered<T>(path: &str, t: &T) -> Vec<u8>
where
    T: Serialize + Schema
{
    let id = Key::for_path::<T>(path);
    let mut header = postcard::to_stdvec(&id).unwrap();
    let body = postcard::to_stdvec(t).unwrap();
    header.extend_from_slice(&body);
    header
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
    map.add_handler::<Pixel>("leds/bottom", bottom::<Pixel>).unwrap();
    map.add_handler::<Rgb>("favorite/color", fav_color::<Rgb>).unwrap();

    let msg = serialize_headered(
        "leds/top",
        &Pixel { position: 3, color: Rgb { r: 10, g: 20, b: 30 } },
    );
    map.dispatch(&msg).unwrap();

    let msg = serialize_headered(
        "leds/bottom",
        &Pixel { position: 3, color: Rgb { r: 10, g: 20, b: 30 } },
    );
    map.dispatch(&msg).unwrap();

    let msg = serialize_headered(
        "favorite/color",
        &Rgb { r: 10, g: 20, b: 30 },
    );
    map.dispatch(&msg).unwrap();

    let msg = serialize_headered(
        "favorite/color",
        &Rgb { r: 10, g: 20, b: 30 },
    );
    println!("{:?}", map.dispatch(&msg));
}

