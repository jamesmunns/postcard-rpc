//! These are items you can use for your error path and error key.
//!
//! This is used by [`define_dispatch!()`][crate::define_dispatch] as well.

use crate::{endpoints, topics, Key, TopicDirection};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

#[cfg(not(feature = "use-std"))]
use postcard_schema::schema::NamedType;

#[cfg(feature = "use-std")]
use postcard_schema::schema::owned::OwnedNamedType;

/// The calculated Key for the type [`WireError`] and the path [`ERROR_PATH`]
pub const ERROR_KEY: Key = Key::for_path::<WireError>(ERROR_PATH);

/// The path string used for the error type
pub const ERROR_PATH: &str = "error";

/// The given frame was too long
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct FrameTooLong {
    /// The length of the too-long frame
    pub len: u32,
    /// The maximum frame length supported
    pub max: u32,
}

/// The given frame was too short
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct FrameTooShort {
    /// The length of the too-short frame
    pub len: u32,
}

/// A protocol error that is handled outside of the normal request type, usually
/// indicating a protocol-level error
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub enum WireError {
    /// The frame exceeded the buffering capabilities of the server
    FrameTooLong(FrameTooLong),
    /// The frame was shorter than the minimum frame size and was rejected
    FrameTooShort(FrameTooShort),
    /// Deserialization of a message failed
    DeserFailed,
    /// Serialization of a message failed, usually due to a lack of space to
    /// buffer the serialized form
    SerFailed,
    /// The key associated with this request was unknown
    UnknownKey,
    /// The server was unable to spawn the associated handler, typically due
    /// to an exhaustion of resources
    FailedToSpawn,
    /// The provided key is below the minimum key size calculated to avoid hash
    /// collisions, and was rejected to avoid potential misunderstanding
    KeyTooSmall,
}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WireError::FrameTooLong(e) => write!(f, "The frame exceeded the buffering capabilities of the server: {} > {}", e.len, e.max),
            WireError::FrameTooShort(e) => write!(f, "The frame was shorter than the minimum frame size and was rejected: {}", e.len),
            WireError::DeserFailed => f.write_str("Deserialization of a message failed"),
            WireError::SerFailed => f.write_str("Serialization of a message failed, usually due to a lack of space to buffer the serialized form"),
            WireError::UnknownKey => f.write_str("The key associated with this request was unknown"),
            WireError::FailedToSpawn => f.write_str("The server was unable to spawn the associated handler, typically due to an exhaustion of resources"),
            WireError::KeyTooSmall => f.write_str("The provided key is below the minimum key size calculated to avoid hash collisions, and was rejected to avoid potential misunderstanding"),
        }
    }
}

impl core::error::Error for WireError {}

/// A single element of schema information
#[cfg(not(feature = "use-std"))]
#[derive(Serialize, Schema, Debug, PartialEq, Copy, Clone)]
pub enum SchemaData<'a> {
    /// A single Type
    Type(&'a NamedType),
    /// A single Endpoint
    Endpoint {
        /// The path of the endpoint
        path: &'a str,
        /// The key of the Request type + path
        request_key: Key,
        /// The key of the Response type + path
        response_key: Key,
    },
    /// A single Topic
    Topic {
        /// The path of the topic
        path: &'a str,
        /// The key of the Message type + path
        key: Key,
        /// The direction of the Topic
        direction: TopicDirection,
    },
}

/// A single element of schema information
#[cfg(feature = "use-std")]
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq, Clone)]
pub enum OwnedSchemaData {
    /// A single Type
    Type(OwnedNamedType),
    /// A single Endpoint
    Endpoint {
        /// The path of the endpoint
        path: String,
        /// The key of the Request type + path
        request_key: Key,
        /// The key of the Response type + path
        response_key: Key,
    },
    /// A single Topic
    Topic {
        /// The path of the topic
        path: String,
        /// The key of the Message type + path
        key: Key,
        /// The direction of the Topic
        direction: TopicDirection,
    },
}

/// A summary of all messages sent when streaming schema data
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq, Copy, Clone)]
pub struct SchemaTotals {
    /// A count of the number of (Owned)SchemaData::Type messages sent
    pub types_sent: u32,
    /// A count of the number of (Owned)SchemaData::Endpoint messages sent
    pub endpoints_sent: u32,
    /// A count of the number of (Owned)SchemaData::Topic messages sent
    pub topics_in_sent: u32,
    /// A count of the number of (Owned)SchemaData::Topic messages sent
    pub topics_out_sent: u32,
    /// A count of the number of messages (any of the above) that failed to send
    pub errors: u32,
}

endpoints! {
    list = STANDARD_ICD_ENDPOINTS;
    omit_std = true;
    | EndpointTy            | RequestTy     | ResponseTy    | Path                       |
    | ----------            | ---------     | ----------    | ----                       |
    | PingEndpoint          | u32           | u32           | "postcard-rpc/ping"        |
    | GetAllSchemasEndpoint | ()            | SchemaTotals  | "postcard-rpc/schemas/get" |
}

topics! {
    list = STANDARD_ICD_TOPICS_OUT;
    direction = crate::TopicDirection::ToClient;
    omit_std = true;
    | TopicTy               | MessageTy         | Path                          | Cfg                           |
    | -------               | ---------         | ----                          | ---                           |
    | GetAllSchemaDataTopic | SchemaData<'a>    | "postcard-rpc/schema/data"    | cfg(not(feature = "use-std")) |
    | GetAllSchemaDataTopic | OwnedSchemaData   | "postcard-rpc/schema/data"    | cfg(feature = "use-std")      |
    | LoggingTopic          | str               | "postcard-rpc/logging"        | cfg(not(feature = "use-std")) |
    | LoggingTopic          | String            | "postcard-rpc/logging"        | cfg(feature = "use-std")      |
}

topics! {
    list = STANDARD_ICD_TOPICS_IN;
    direction = crate::TopicDirection::ToServer;
    omit_std = true;
    | TopicTy           | MessageTy         | Path                          | Cfg                           |
    | -------           | ---------         | ----                          | ---                           |
}
