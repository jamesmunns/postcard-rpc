//! The goal of `postcard-rpc` is to make it easier for a
//! host PC to talk to a constrained device, like a microcontroller.
//!
//! See [the repo] for examples
//!
//! [the repo]: https://github.com/jamesmunns/postcard-rpc
//! [the overview]: https://github.com/jamesmunns/postcard-rpc/blob/main/docs/overview.md
//!
//! ## Defining a schema
//!
//! Typically, you will define your "wire types" in a shared schema crate. This
//! crate essentially defines the protocol used between two or more devices.
//!
//! A schema consists of a couple of necessary items:
//!
//! ### Wire types
//!
//! We will need to define all of the types that we will use within our protocol.
//! We specify normal Rust types, which will need to implement or derive three
//! important traits:
//!
//! * [`serde`]'s [`Serialize`] trait - which defines how we can
//!   convert a type into bytes on the wire
//! * [`serde`]'s [`Deserialize`] trait - which defines how we
//!   can convert bytes on the wire into a type
//! * [`postcard-schema`]'s [`Schema`] trait - which generates a reflection-style
//!   schema value for a given type.
//!
//! ### Endpoints
//!
//! Now that we have some basic types that will be used on the wire, we need
//! to start building our protocol. The first thing we can build are [Endpoint]s,
//! which represent a bidirectional "Request"/"Response" relationship. One of our
//! devices will act as a Client (who makes a request, and receives a response),
//! and the other device will act as a Server (who receives a request, and sends
//! a response). Every request should be followed (eventually) by exactly one response.
//!
//! An endpoint consists of:
//!
//! * The type of the Request
//! * The type of the Response
//! * A string "path", like an HTTP URI that uniquely identifies the endpoint.
//!
//! ### Topics
//!
//! Sometimes, you would just like to send data in a single direction, with no
//! response. This could be for reasons like asynchronous logging, blindly sending
//! sensor data periodically, or any other reason you can think of.
//!
//! Topics have no "client" or "server" role, either device may decide to send a
//! message on a given topic.
//!
//! A topic consists of:
//!
//! * The type of the Message
//! * A string "path", like an HTTP URI that uniquely identifies the topic.

#![cfg_attr(not(any(test, feature = "use-std")), no_std)]

use header::VarKey;
// use headered::extract_header_from_bytes;
use postcard_schema::{schema::NamedType, Schema};
use serde::{Deserialize, Serialize};

#[cfg(feature = "cobs")]
pub mod accumulator;

pub mod hash;
pub mod header;

#[cfg(feature = "use-std")]
pub mod host_client;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

mod macros;

pub mod server;

pub mod uniques;

/// The `Key` uniquely identifies what "kind" of message this is.
///
/// In order to generate it, `postcard-rpc` takes two pieces of data:
///
/// * a `&str` "path" URI, similar to how you would use URIs as part of an HTTP path
/// * The schema of the message type itself, using the experimental [schema] feature of `postcard`.
///
/// [schema]: https://docs.rs/postcard/latest/postcard/experimental/index.html#message-schema-generation
///
/// Specifically, we use [`Fnv1a`](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function),
/// and produce a 64-bit digest, by first hashing the path, then hashing the
/// schema. Fnv1a is a non-cryptographic hash function, designed to be reasonably
/// efficient to compute even on small platforms like microcontrollers.
///
/// Changing **anything** about *either* of the path or the schema will produce
/// a drastically different `Key` value.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize, Hash, Schema)]
pub struct Key([u8; 8]);

impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Key(")?;
        for b in self.0.iter() {
            f.write_fmt(format_args!("{} ", b))?;
        }
        f.write_str(")")
    }
}

impl Key {
    /// Create a Key for the given type and path
    pub const fn for_path<T>(path: &str) -> Self
    where
        T: Schema + ?Sized,
    {
        Key(crate::hash::fnv1a64::hash_ty_path::<T>(path))
    }

    /// Unsafely create a key from a given 8-byte value
    ///
    /// ## Safety
    ///
    /// This MUST only be used with pre-calculated values. Incorrectly
    /// created keys could lead to the improper deserialization of
    /// messages.
    pub const unsafe fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    /// Extract the bytes making up this key
    pub const fn to_bytes(&self) -> [u8; 8] {
        self.0
    }

    /// Compare 2 keys in const context.
    pub const fn const_cmp(&self, other: &Self) -> bool {
        let mut i = 0;
        while i < self.0.len() {
            if self.0[i] != other.0[i] {
                return false;
            }

            i += 1;
        }

        true
    }
}

#[cfg(feature = "use-std")]
mod key_owned {
    use super::*;
    use postcard_schema::schema::owned::OwnedNamedType;
    impl Key {
        pub fn for_owned_schema_path(path: &str, nt: &OwnedNamedType) -> Key {
            Key(crate::hash::fnv1a64_owned::hash_ty_path_owned(path, nt))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Key4(pub [u8; 4]);
#[derive(Debug, Copy, Clone)]
pub struct Key2(pub [u8; 2]);
#[derive(Debug, Copy, Clone)]
pub struct Key1(pub u8);

impl Key1 {
    pub const fn from_key2(value: Key2) -> Self {
        let [a, b] = value.0;
        Self(a ^ b)
    }

    pub const fn from_key4(value: Key4) -> Self {
        let [a, b, c, d] = value.0;
        Self(a ^ b ^ c ^ d)
    }

    pub const fn from_key8(value: Key) -> Self {
        let [a, b, c, d, e, f, g, h] = value.0;
        Self(a ^ b ^ c ^ d ^ e ^ f ^ g ^ h)
    }

    pub const fn to_bytes(&self) -> u8 {
        self.0
    }

    pub fn try_from_varkey(value: &VarKey) -> Option<Self> {
        Some(match value {
            VarKey::Key1(key1) => *key1,
            VarKey::Key2(key2) => Key1::from_key2(*key2),
            VarKey::Key4(key4) => Key1::from_key4(*key4),
            VarKey::Key8(key) => Key1::from_key8(*key),
        })
    }
}

impl Key2 {
    pub const fn from_key4(value: Key4) -> Self {
        let [a, b, c, d] = value.0;
        Self([a ^ b, c ^ d])
    }

    pub const fn from_key8(value: Key) -> Self {
        let [a, b, c, d, e, f, g, h] = value.0;
        Self([a ^ b ^ c ^ d, e ^ f ^ g ^ h])
    }

    pub const fn to_bytes(&self) -> [u8; 2] {
        self.0
    }

    pub fn try_from_varkey(value: &VarKey) -> Option<Self> {
        Some(match value {
            VarKey::Key1(_) => return None,
            VarKey::Key2(key2) => *key2,
            VarKey::Key4(key4) => Key2::from_key4(*key4),
            VarKey::Key8(key) => Key2::from_key8(*key),
        })
    }
}

impl Key4 {
    pub const fn from_key8(value: Key) -> Self {
        let [a, b, c, d, e, f, g, h] = value.0;
        Self([a ^ b, c ^ d, e ^ f, g ^ h])
    }

    pub const fn to_bytes(&self) -> [u8; 4] {
        self.0
    }

    pub fn try_from_varkey(value: &VarKey) -> Option<Self> {
        Some(match value {
            VarKey::Key1(_) => return None,
            VarKey::Key2(_) => return None,
            VarKey::Key4(key4) => *key4,
            VarKey::Key8(key) => Key4::from_key8(*key),
        })
    }
}

impl Key {
    pub const fn from_key8(value: Key) -> Self {
        value
    }

    pub fn try_from_varkey(value: &VarKey) -> Option<Self> {
        match value {
            VarKey::Key8(key) => Some(*key),
            _ => None,
        }
    }
}

impl From<Key2> for Key1 {
    fn from(value: Key2) -> Self {
        Self::from_key2(value)
    }
}

impl From<Key4> for Key1 {
    fn from(value: Key4) -> Self {
        Self::from_key4(value)
    }
}

impl From<Key> for Key1 {
    fn from(value: Key) -> Self {
        Self::from_key8(value)
    }
}

impl From<Key4> for Key2 {
    fn from(value: Key4) -> Self {
        Self::from_key4(value)
    }
}

impl From<Key> for Key2 {
    fn from(value: Key) -> Self {
        Self::from_key8(value)
    }
}

impl From<Key> for Key4 {
    fn from(value: Key) -> Self {
        Self::from_key8(value)
    }
}

/// A marker trait denoting a single endpoint
///
/// Typically used with the [endpoint] macro.
pub trait Endpoint {
    /// The type of the Request (client to server)
    type Request: Schema;
    /// The type of the Response (server to client)
    type Response: Schema;
    /// The path associated with this Endpoint
    const PATH: &'static str;
    /// The unique [Key] identifying the Request
    const REQ_KEY: Key;
    /// The unique [Key] identifying the Response
    const RESP_KEY: Key;
}

/// A marker trait denoting a single topic
///
/// Unlike [Endpoint]s, [Topic]s are unidirectional, and can be sent
/// at any time asynchronously. Messages may be sent client to server,
/// or server to client.
///
/// Typically used with the [topic] macro.
pub trait Topic {
    /// The type of the Message (unidirectional)
    type Message: Schema + ?Sized;
    /// The path associated with this Topic
    const PATH: &'static str;
    /// The unique [Key] identifying the Message
    const TOPIC_KEY: Key;
}

/// These are items you can use for your error path and error key.
///
/// This is used by [`define_dispatch!()`] as well.
pub mod standard_icd {
    use crate::Key;
    use postcard_schema::Schema;
    use serde::{Deserialize, Serialize};

    pub const ERROR_KEY: Key = Key::for_path::<WireError>(ERROR_PATH);
    pub const ERROR_PATH: &str = "error";

    #[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
    pub struct FrameTooLong {
        pub len: u32,
        pub max: u32,
    }

    #[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
    pub struct FrameTooShort {
        pub len: u32,
    }

    #[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
    pub enum WireError {
        FrameTooLong(FrameTooLong),
        FrameTooShort(FrameTooShort),
        DeserFailed,
        SerFailed,
        // TODO: report different keys lens?
        UnknownKey,
        FailedToSpawn,
        KeyTooSmall,
    }

    #[cfg(not(feature = "use-std"))]
    crate::topic!(Logging, [u8], "logs/formatted");

    #[cfg(feature = "use-std")]
    crate::topic!(Logging, Vec<u8>, "logs/formatted");
}

pub struct DeviceMap {
    pub types: &'static [&'static NamedType],
    pub endpoints: &'static [(&'static str, Key, Key)],
    pub topics: &'static [(&'static str, Key)],
}

#[derive(Debug)]
pub struct EndpointMap {
    pub types: &'static [&'static NamedType],
    pub endpoints: &'static [(&'static str, Key, Key)],
}

#[derive(Debug)]
pub struct TopicMap {
    pub types: &'static [&'static NamedType],
    pub topics: &'static [(&'static str, Key)],
}
