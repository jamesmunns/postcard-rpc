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
#![deny(missing_docs)]
#![deny(unused_imports)]
#![deny(rustdoc::broken_intra_doc_links)]

/// Re-export used by macros
#[doc(hidden)]
pub use postcard;
/// Re-export used by macros
#[doc(hidden)]
pub use postcard_schema;

use header::{VarKey, VarKeyKind};
use postcard_schema::{schema::NamedType, Schema};
use serde::{Deserialize, Serialize};

pub mod header;
mod macros;
pub mod server;
pub mod standard_icd;
pub mod uniques;

#[cfg(feature = "cobs")]
pub mod accumulator;

#[cfg(feature = "use-std")]
pub mod host_client;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

// Re-export Key components that now live in postcard-schema instead
// of here in postcard-rpc
pub use postcard_schema::key::hash;
pub use postcard_schema::key::Key;

/// A compacted 2-byte key
///
/// This is defined specifically as the following conversion:
///
/// * Key8 bytes (`[u8; 8]`): `[a, b, c, d, e, f, g, h]`
/// * Key4 bytes (`u8`): `a ^ b ^ c ^ d ^ e ^ f ^ g ^ h`
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Key1(u8);

/// A compacted 2-byte key
///
/// This is defined specifically as the following conversion:
///
/// * Key8 bytes (`[u8; 8]`): `[a, b, c, d, e, f, g, h]`
/// * Key4 bytes (`[u8; 2]`): `[a ^ b ^ c ^ d, e ^ f ^ g ^ h]`
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Key2([u8; 2]);

/// A compacted 4-byte key
///
/// This is defined specifically as the following conversion:
///
/// * Key8 bytes (`[u8; 8]`): `[a, b, c, d, e, f, g, h]`
/// * Key4 bytes (`[u8; 4]`): `[a ^ b, c ^ d, e ^ f, g ^ h]`
#[derive(Debug, Copy, Clone, PartialEq)]
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
        let [a, b, c, d, e, f, g, h] = value.to_bytes();
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
        let [a, b, c, d, e, f, g, h] = value.to_bytes();
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
        let [a, b, c, d, e, f, g, h] = value.to_bytes();
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

/// The source type was too small to create from
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct TooSmall;

impl TryFrom<&VarKey> for Key1 {
    type Error = TooSmall;

    #[inline]
    fn try_from(value: &VarKey) -> Result<Self, Self::Error> {
        Self::try_from_varkey(value).ok_or(TooSmall)
    }
}

impl TryFrom<&VarKey> for Key2 {
    type Error = TooSmall;

    #[inline]
    fn try_from(value: &VarKey) -> Result<Self, Self::Error> {
        Self::try_from_varkey(value).ok_or(TooSmall)
    }
}

impl TryFrom<&VarKey> for Key4 {
    type Error = TooSmall;

    #[inline]
    fn try_from(value: &VarKey) -> Result<Self, Self::Error> {
        Self::try_from_varkey(value).ok_or(TooSmall)
    }
}

impl TryFrom<&VarKey> for Key {
    type Error = TooSmall;

    #[inline]
    fn try_from(value: &VarKey) -> Result<Self, Self::Error> {
        if let VarKey::Key8(key) = value {
            Ok(*key)
        } else {
            Err(TooSmall)
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
    /// The unique [Key4] identifying the Request
    const REQ_KEY4: Key4 = Key4::from_key8(Self::REQ_KEY);
    /// The unique [Key2] identifying the Request
    const REQ_KEY2: Key2 = Key2::from_key8(Self::REQ_KEY);
    /// The unique [Key1] identifying the Request
    const REQ_KEY1: Key1 = Key1::from_key8(Self::REQ_KEY);
    /// The unique [Key] identifying the Response
    const RESP_KEY: Key;
    /// The unique [Key4] identifying the Response
    const RESP_KEY4: Key4 = Key4::from_key8(Self::RESP_KEY);
    /// The unique [Key2] identifying the Response
    const RESP_KEY2: Key2 = Key2::from_key8(Self::RESP_KEY);
    /// The unique [Key1] identifying the Response
    const RESP_KEY1: Key1 = Key1::from_key8(Self::RESP_KEY);
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
    /// The unique [Key4] identifying the Message
    const TOPIC_KEY4: Key4 = Key4::from_key8(Self::TOPIC_KEY);
    /// The unique [Key2] identifying the Message
    const TOPIC_KEY2: Key2 = Key2::from_key8(Self::TOPIC_KEY);
    /// The unique [Key2] identifying the Message
    const TOPIC_KEY1: Key1 = Key1::from_key8(Self::TOPIC_KEY);
}

/// The direction of topic messages
#[derive(Debug, PartialEq, Clone, Copy, Schema, Serialize, Deserialize)]
pub enum TopicDirection {
    /// Topic messages sent TO the SERVER, FROM the CLIENT
    ToServer,
    /// Topic messages sent TO the CLIENT, FROM the SERVER
    ToClient,
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
    /// The direction of these topic messages
    pub direction: TopicDirection,
    /// The set of unique types used by all topics in this map
    pub types: &'static [&'static NamedType],
    /// The list of topics by path string and topic key
    pub topics: &'static [(&'static str, Key)],
}
