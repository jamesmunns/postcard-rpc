#![no_std]

use postcard::experimental::schema::Schema;
use serde::{Serialize, Deserialize};

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct Sleep {
    pub seconds: u32,
    pub micros: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct SleepDone {
    pub slept_for: Sleep,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub enum FatalError {
    UnknownEndpoint,
    NotEnoughSenders,
    WireFailure,
}
