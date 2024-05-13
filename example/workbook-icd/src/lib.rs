#![no_std]

use postcard::experimental::schema::Schema;
use postcard_rpc::{endpoint, topic};
use serde::{Deserialize, Serialize};

endpoint!(PingEndpoint, u32, u32, "ping");

// ---

endpoint!(GetUniqueIdEndpoint, (), u64, "unique_id/get");

endpoint!(SetSingleLedEndpoint, SingleLed, Result<(), BadPositionError>, "led/set_one");
endpoint!(SetAllLedEndpoint, [Rgb8; 24], (), "led/set_all");

endpoint!(StartAccelerationEndpoint, StartAccel, (), "accel/start");
endpoint!(StopAccelerationEndpoint, (), bool, "accel/stop");

topic!(AccelTopic, Acceleration, "accel/data");

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SingleLed {
    pub position: u32,
    pub rgb: Rgb8,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq, Copy, Clone)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct BadPositionError;

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct Acceleration {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub enum AccelRange {
    G2,
    G4,
    G8,
    G16,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct StartAccel {
    pub interval_ms: u32,
    pub range: AccelRange,
}
