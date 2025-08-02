use postcard_rpc::{endpoints, topics, TopicDirection};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Schema)]
pub struct SleepMillis {
    pub millis: u16,
}

#[derive(Debug, Serialize, Deserialize, Schema)]
pub struct SleptMillis {
    pub millis: u16,
}

#[derive(Debug, Serialize, Deserialize, Schema)]
pub enum LedState {
    Off,
    On,
}

// ---

// Endpoints spoken by our device
//
// GetUniqueIdEndpoint is mandatory, the others are examples
endpoints! {
    list = ENDPOINT_LIST;
    | EndpointTy                | RequestTy             | ResponseTy            | Path                          | Cfg                           |
    | ----------                | ---------             | ----------            | ----                          | ---                           |
    | GetUniqueIdEndpoint       | ()                    | u64                   | "poststation/unique_id/get"   |                               |
    | RebootToPicoBoot          | ()                    | ()                    | "template/picoboot/reset"     |                               |
    | SleepEndpoint             | SleepMillis           | SleptMillis           | "template/sleep"              |                               |
    | SetLedEndpoint            | LedState              | ()                    | "template/led/set"            |                               |
    | GetLedEndpoint            | ()                    | LedState              | "template/led/get"            |                               |
}

// incoming topics handled by our device
topics! {
    list = TOPICS_IN_LIST;
    direction = TopicDirection::ToServer;
    | TopicTy                   | MessageTy     | Path              |
    | -------                   | ---------     | ----              |
}

// outgoing topics handled by our device
topics! {
    list = TOPICS_OUT_LIST;
    direction = TopicDirection::ToClient;
    | TopicTy                   | MessageTy     | Path              | Cfg                           |
    | -------                   | ---------     | ----              | ---                           |
}
