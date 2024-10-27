//! The goal of `postcard-rpc` is to make it easier for a
//! host PC to talk to a constrained device, like a microcontroller.
//!
//! See [the repo] for examples
//!
//! [the repo]: https://github.com/jamesmunns/postcard-rpc
//! [the overview]: https://github.com/jamesmunns/postcard-rpc/blob/main/docs/overview.md
//!
//! ## Architecture overview
//!
//! ```text
//!                 ┌──────────┐      ┌─────────┐         ┌───────────┐
//!                 │ Endpoint │      │ Publish │         │ Subscribe │
//!                 └──────────┘      └─────────┘         └───────────┘
//!                   │     ▲       message│                │        ▲
//!    ┌────────┐ rqst│     │resp          │       subscribe│        │messages
//!  ┌─┤ CLIENT ├─────┼─────┼──────────────┼────────────────┼────────┼──┐
//!  │ └────────┘     ▼     │              ▼                ▼        │  │
//!  │       ┌─────────────────────────────────────────────────────┐ │  │
//!  │       │                     HostClient                      │ │  │
//!  │       └─────────────────────────────────────────────────────┘ │  │
//!  │         │                  │              ▲           │       |  │
//!  │         │                  │              │           │       │  │
//!  │         │                  │              │           ▼       │  │
//!  │         │                  │      ┌──────────────┬──────────────┐│
//!  │         │                  └─────▶│ Pending Resp │ Subscription ││
//!  │         │                         └──────────────┴──────────────┘│
//!  │         │                                 ▲              ▲       │
//!  │         │                                 └───────┬──────┘       │
//!  │         ▼                                         │              │
//!  │      ┌────────────────────┐            ┌────────────────────┐    │
//!  │      ││ Task: out_worker  │            │  Task: in_worker  ▲│    │
//!  │      ├┼───────────────────┤            ├───────────────────┼┤    │
//!  │      │▼  Trait: WireTx    │            │   Trait: WireRx   ││    │
//!  └──────┴────────────────────┴────────────┴────────────────────┴────┘
//!                    │ ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐ ▲
//!                    │   The Server + Client WireRx    │
//!                    │ │ and WireTx traits can be    │ │
//!                    │   impl'd for any wire           │
//!                    │ │ transport like USB, TCP,    │ │
//!                    │   I2C, UART, etc.               │
//!                    ▼ └ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘ │
//!   ┌─────┬────────────────────┬────────────┬────────────────────┬─────┐
//!   │     ││  Trait: WireRx    │            │   Trait: WireTx   ▲│     │
//!   │     ├┼───────────────────┤            ├───────────────────┼┤     │
//!   │     ││      Server       │       ┌───▶│       Sender      ││     │
//!   │     ├┼───────────────────┤       │    └────────────────────┘     │
//!   │     │▼ Macro: Dispatch   │       │               ▲               │
//!   │     └────────────────────┘       │               │               │
//!   │    ┌─────────┐ │ ┌──────────┐    │ ┌───────────┐ │ ┌───────────┐ │
//!   │    │  Topic  │ │ │ Endpoint │    │ │ Publisher │ │ │ Publisher │ │
//!   │    │   fn    │◀┼▶│ async fn │────┤ │   Task    │─┼─│   Task    │ │
//!   │    │ Handler │ │ │ Handler  │    │ └───────────┘ │ └───────────┘ │
//!   │    └─────────┘ │ └──────────┘    │               │               │
//!   │    ┌─────────┐ │ ┌──────────┐    │ ┌───────────┐ │ ┌───────────┐ │
//!   │    │  Topic  │ │ │ Endpoint │    │ │ Publisher │ │ │ Publisher │ │
//!   │    │async fn │◀┴▶│   task   │────┘ │   Task    │─┴─│   Task    │ │
//!   │    │ Handler │   │ Handler  │      └───────────┘   └───────────┘ │
//!   │    └─────────┘   └──────────┘                                    │
//!   │ ┌────────┐                                                       │
//!   └─┤ SERVER ├───────────────────────────────────────────────────────┘
//!     └────────┘
//! ```
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
//! * [`postcard_schema`]'s [`Schema`] trait - which generates a reflection-style
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
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

use header::{VarKey, VarKeyKind};
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
        /// Calculate the Key for the given path and [`OwnedNamedType`]
        pub fn for_owned_schema_path(path: &str, nt: &OwnedNamedType) -> Key {
            Key(crate::hash::fnv1a64_owned::hash_ty_path_owned(path, nt))
        }
    }
}

/// A compacted 2-byte key
///
/// This is defined specifically as the following conversion:
///
/// * Key8 bytes (`[u8; 8]`): `[a, b, c, d, e, f, g, h]`
/// * Key4 bytes (`u8`): `a ^ b ^ c ^ d ^ e ^ f ^ g ^ h`
#[derive(Debug, Copy, Clone)]
pub struct Key1(u8);

/// A compacted 2-byte key
///
/// This is defined specifically as the following conversion:
///
/// * Key8 bytes (`[u8; 8]`): `[a, b, c, d, e, f, g, h]`
/// * Key4 bytes (`[u8; 2]`): `[a ^ b ^ c ^ d, e ^ f ^ g ^ h]`
#[derive(Debug, Copy, Clone)]
pub struct Key2([u8; 2]);

/// A compacted 4-byte key
///
/// This is defined specifically as the following conversion:
///
/// * Key8 bytes (`[u8; 8]`): `[a, b, c, d, e, f, g, h]`
/// * Key4 bytes (`[u8; 4]`): `[a ^ b, c ^ d, e ^ f, g ^ h]`
#[derive(Debug, Copy, Clone)]
pub struct Key4([u8; 4]);

impl Key1 {
    /// Convert from a 2-byte key
    ///
    /// This is a lossy conversion, and can never fail
    #[inline]
    pub const fn from_key2(value: Key2) -> Self {
        let [a, b] = value.0;
        Self(a ^ b)
    }

    /// Convert from a 4-byte key
    ///
    /// This is a lossy conversion, and can never fail
    #[inline]
    pub const fn from_key4(value: Key4) -> Self {
        let [a, b, c, d] = value.0;
        Self(a ^ b ^ c ^ d)
    }

    /// Convert from a full size 8-byte key
    ///
    /// This is a lossy conversion, and can never fail
    #[inline]
    pub const fn from_key8(value: Key) -> Self {
        let [a, b, c, d, e, f, g, h] = value.0;
        Self(a ^ b ^ c ^ d ^ e ^ f ^ g ^ h)
    }

    /// Convert to the inner byte representation
    #[inline]
    pub const fn to_bytes(&self) -> u8 {
        self.0
    }

    /// Create a `Key1` from a [`VarKey`]
    ///
    /// This method can never fail, but has the same API as other key
    /// types for consistency reasons.
    #[inline]
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
    /// Convert from a 4-byte key
    ///
    /// This is a lossy conversion, and can never fail
    #[inline]
    pub const fn from_key4(value: Key4) -> Self {
        let [a, b, c, d] = value.0;
        Self([a ^ b, c ^ d])
    }

    /// Convert from a full size 8-byte key
    ///
    /// This is a lossy conversion, and can never fail
    #[inline]
    pub const fn from_key8(value: Key) -> Self {
        let [a, b, c, d, e, f, g, h] = value.0;
        Self([a ^ b ^ c ^ d, e ^ f ^ g ^ h])
    }

    /// Convert to the inner byte representation
    #[inline]
    pub const fn to_bytes(&self) -> [u8; 2] {
        self.0
    }

    /// Attempt to create a [`Key2`] from a [`VarKey`].
    ///
    /// Only succeeds if `value` is a `VarKey::Key2`, `VarKey::Key4`, or `VarKey::Key8`.
    #[inline]
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
    /// Convert from a full size 8-byte key
    ///
    /// This is a lossy conversion, and can never fail
    #[inline]
    pub const fn from_key8(value: Key) -> Self {
        let [a, b, c, d, e, f, g, h] = value.0;
        Self([a ^ b, c ^ d, e ^ f, g ^ h])
    }

    /// Convert to the inner byte representation
    #[inline]
    pub const fn to_bytes(&self) -> [u8; 4] {
        self.0
    }

    /// Attempt to create a [`Key4`] from a [`VarKey`].
    ///
    /// Only succeeds if `value` is a `VarKey::Key4` or `VarKey::Key8`.
    #[inline]
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
    /// This is an identity function, used for consistency
    #[inline]
    pub const fn from_key8(value: Key) -> Self {
        value
    }

    /// Attempt to create a [`Key`] from a [`VarKey`].
    ///
    /// Only succeeds if `value` is a `VarKey::Key8`.
    #[inline]
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

    #[cfg(not(feature = "use-std"))]
    crate::topic!(Logging, [u8], "logs/formatted");

    #[cfg(feature = "use-std")]
    crate::topic!(Logging, Vec<u8>, "logs/formatted");
}

/// An overview of all topics (in and out) and endpoints
///
/// Typically generated by the [`define_dispatch!()`] macro. Contains a list
/// of all unique types across endpoints and topics, as well as the endpoints,
/// topics in (client to server), topics out (server to client), as well as a
/// calculated minimum key length required to avoid collisions in either the in
/// or out direction.
pub struct DeviceMap {
    /// The set of unique types used by all endpoints and topics in this map
    pub types: &'static [&'static NamedType],
    /// The list of endpoints by path string, request key, and response key
    pub endpoints: &'static [(&'static str, Key, Key)],
    /// The list of topics (client to server) by path string and topic key
    pub topics_in: &'static [(&'static str, Key)],
    /// The list of topics (server to client) by path string and topic key
    pub topics_out: &'static [(&'static str, Key)],
    /// The minimum key size required to avoid hash collisions
    pub min_key_len: VarKeyKind,
}

/// An overview of a list of endpoints
///
/// Typically generated by the [`endpoints!()`] macro. Contains a list of
/// all unique types used by a list of endpoints, as well as the list of these
/// endpoints by path, request key, and response key
#[derive(Debug)]
pub struct EndpointMap {
    /// The set of unique types used by all endpoints in this map
    pub types: &'static [&'static NamedType],
    /// The list of endpoints by path string, request key, and response key
    pub endpoints: &'static [(&'static str, Key, Key)],
}

/// An overview of a list of topics
///
/// Typically generated by the [`topics!()`] macro. Contains a list of all
/// unique types used by a list of topics as well as the list of the topics
/// by path and key
#[derive(Debug)]
pub struct TopicMap {
    /// The set of unique types used by all topics in this map
    pub types: &'static [&'static NamedType],
    /// The list of topics by path string and topic key
    pub topics: &'static [(&'static str, Key)],
}
