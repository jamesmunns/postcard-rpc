#![no_std]

use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};

pub mod sleep {
    use postcard_rpc::endpoint;

    use super::*;

    endpoint!(SleepEndpoint, Sleep, SleepDone, "done");

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
    use postcard_rpc::Key;

    use super::*;

    pub const ERROR_PATH: &str = "error";
    pub const ERROR_KEY: Key = Key::for_path::<FatalError>(ERROR_PATH);

    #[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
    pub enum FatalError {
        UnknownEndpoint,
        NotEnoughSenders,
        WireFailure,
    }
}
