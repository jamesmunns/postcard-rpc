#![cfg_attr(not(feature = "use-std"), no_std)]

use postcard_rpc::{endpoints, topics, TopicDirection};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

// ---

pub type SingleLedSetResult = Result<(), BadPositionError>;
pub type AllLedArray = [Rgb8; 24];

endpoints! {
    list = ENDPOINT_LIST;
    | EndpointTy                | RequestTy     | ResponseTy            | Path              |
    | ----------                | ---------     | ----------            | ----              |
    | PingEndpoint              | u32           | u32                   | "ping"            |
    | GetUniqueIdEndpoint       | ()            | u64                   | "unique_id/get"   |
    | SetSingleLedEndpoint      | SingleLed     | SingleLedSetResult    | "led/set_one"     |
    | SetAllLedEndpoint         | AllLedArray   | ()                    | "led/set_all"     |
    | StartAccelerationEndpoint | StartAccel    | ()                    | "accel/start"     |
    | StopAccelerationEndpoint  | ()            | bool                  | "accel/stop"      |
}

topics! {
    list = TOPICS_IN_LIST;
    direction = TopicDirection::ToServer;
    | TopicTy                   | MessageTy     | Path              |
    | -------                   | ---------     | ----              |
}

topics! {
    list = TOPICS_OUT_LIST;
    direction = TopicDirection::ToClient;
    | TopicTy                   | MessageTy     | Path              | Cfg                           |
    | -------                   | ---------     | ----              | ---                           |
    | AccelTopic                | Acceleration  | "accel/data"      |                               |
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SingleLed {
    pub position: u32,
    pub rgb: Rgb8,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq, Copy, Clone)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
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
