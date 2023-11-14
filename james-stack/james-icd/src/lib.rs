#![no_std]

use postcard::experimental::schema::Schema;
use serde::{Serialize, Deserialize};

pub mod sleep {
    use super::*;

    pub const SLEEP_PATH: &str = "sleep";

    #[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
    pub struct Sleep {
        pub seconds: u32,
        pub micros: u32,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
    pub struct SleepDone {
        pub slept_for: Sleep,
    }
}

pub mod wire_error {
    use super::*;

    pub const ERROR_PATH: &str = "error";

    #[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
    pub enum FatalError {
        UnknownEndpoint,
        NotEnoughSenders,
        WireFailure,
    }
}
