//! The goal of `postcard-rpc` is to make it easier for a
//! host PC to talk to a constrained device, like a microcontroller.
//!
//! See [the repo] for examples, and [the overview] for more details on how
//! to use this crate.
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
//! * [`postcard`]'s [`Schema`] trait - which generates a reflection-style
//!   schema value for a given type.
//!
//! Here's an example of three types we'll use in future examples:
//!
//! ```rust
//! // Consider making your shared "wire types" crate conditionally no-std,
//! // if you want to use it with no-std embedded targets! This makes it no_std
//! // except for testing and when the "use-std" feature is active.
//! //
//! // You may need to also ensure that `std`/`use-std` features are not active
//! // in any dependencies as well.
//! #![cfg_attr(not(any(test, feature = "use-std")), no_std)]
//! # fn main() {}
//!
//! use serde::{Serialize, Deserialize};
//! use postcard::experimental::schema::Schema;
//!
//! #[derive(Serialize, Deserialize, Schema)]
//! pub struct Alpha {
//!     pub one: u8,
//!     pub two: i64,
//! }
//!
//! #[derive(Serialize, Deserialize, Schema)]
//! pub enum Beta {
//!     Bib,
//!     Bim(i16),
//!     Bap,
//! }
//!
//! #[derive(Serialize, Deserialize, Schema)]
//! pub struct Delta(pub [u8; 32]);
//!
//! #[derive(Serialize, Deserialize, Schema)]
//! pub enum WireError {
//!     ALittleBad,
//!     VeryBad,
//! }
//! ```
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
//! The easiest way to define an Endpoint is to use the [`endpoint!`][endpoint]
//! macro.
//!
//! ```rust
//! # use serde::{Serialize, Deserialize};
//! # use postcard::experimental::schema::Schema;
//! #
//! # #[derive(Serialize, Deserialize, Schema)]
//! # pub struct Alpha {
//! #     pub one: u8,
//! #     pub two: i64,
//! # }
//! #
//! # #[derive(Serialize, Deserialize, Schema)]
//! # pub enum Beta {
//! #     Bib,
//! #     Bim(i16),
//! #     Bap,
//! # }
//! #
//! use postcard_rpc::endpoint;
//!
//! // Define an endpoint
//! endpoint!(
//!     // This is the name of a marker type that represents our Endpoint,
//!     // and implements the `Endpoint` trait.
//!     FirstEndpoint,
//!     // This is the request type for this endpoint
//!     Alpha,
//!     // This is the response type for this endpoint
//!     Beta,
//!     // This is the path/URI of the endpoint
//!     "endpoints/first",
//! );
//! ```
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
//!
//! The easiest way to define a Topic is to use the [`topic!`][topic]
//! macro.
//!
//! ```rust
//! # use serde::{Serialize, Deserialize};
//! # use postcard::experimental::schema::Schema;
//! #
//! # #[derive(Serialize, Deserialize, Schema)]
//! # pub struct Delta(pub [u8; 32]);
//! #
//! use postcard_rpc::topic;
//!
//! // Define a topic
//! topic!(
//!     // This is the name of a marker type that represents our Topic,
//!     // and implements `Topic` trait.
//!     FirstTopic,
//!     // This is the message type for the endpoint (note there is no
//!     // response type!)
//!     Delta,
//!     // This is the path/URI of the topic
//!     "topics/first",
//! );
//! ```
//!
//! ## Using a schema
//!
//! At the moment, this library is primarily oriented around:
//!
//! * A single Client, usually a PC, with access to `std`
//! * A single Server, usually an MCU, without access to `std`
//!
//! For Client facilities, check out the [`host_client`] module,
//! particularly the [`HostClient`][host_client::HostClient] struct.
//! This is only available with the `use-std` feature active.
//!
//! A serial-port transport using cobs encoding is available with the `cobs-serial` feature.
//! This feature will add the [`new_serial_cobs`][host_client::HostClient::new_serial_cobs] constructor to [`HostClient`][host_client::HostClient].
//!
//! For Server facilities, check out the [`Dispatch`] struct. This is
//! available with or without the standard library.

#![cfg_attr(not(any(test, feature = "use-std")), no_std)]

use headered::extract_header_from_bytes;
use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};

#[cfg(feature = "cobs")]
pub mod accumulator;

pub mod hash;
pub mod headered;

#[cfg(feature = "use-std")]
pub mod host_client;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

#[cfg(feature = "embassy-usb-0_3-server")]
pub mod target_server;

mod macros;

/// Error type for [Dispatch]
#[derive(Debug, PartialEq)]
pub enum Error<E> {
    /// No handler was found for the given message.
    /// The decoded key and sequence number are returned
    NoMatchingHandler { key: Key, seq_no: u32 },
    /// The handler returned an error
    DispatchFailure(E),
    /// An error when decoding messages
    Postcard(postcard::Error),
}

impl<E> From<postcard::Error> for Error<E> {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}

/// Dispatch is the primary interface for MCU "server" devices.
///
/// Dispatch is generic over three types:
///
/// 1. The `Context`, which will be passed as a mutable reference
///    to each of the handlers. It typically should contain
///    whatever resource is necessary to send replies back to
///    the host.
/// 2. The `Error` type, which can be returned by handlers
/// 3. `N`, for the maximum number of handlers
///
/// If you plan to use COBS encoding, you can also use [CobsDispatch].
/// which will automatically handle accumulating bytes from the wire.
///
/// [CobsDispatch]: crate::accumulator::dispatch::CobsDispatch
/// Note: This will be available when the `cobs` or `cobs-serial` feature is enabled.
pub struct Dispatch<Context, Error, const N: usize> {
    items: heapless::Vec<(Key, Handler<Context, Error>), N>,
    context: Context,
}

impl<Context, Err, const N: usize> Dispatch<Context, Err, N> {
    /// Create a new [Dispatch]
    pub fn new(c: Context) -> Self {
        Self {
            items: heapless::Vec::new(),
            context: c,
        }
    }

    /// Add a handler to the [Dispatch] for the given path and type
    ///
    /// Returns an error if the given type+path have already been added,
    /// or if Dispatch is full.
    pub fn add_handler<E: Endpoint>(
        &mut self,
        handler: Handler<Context, Err>,
    ) -> Result<(), &'static str> {
        if self.items.is_full() {
            return Err("full");
        }
        let id = E::REQ_KEY;
        if self.items.iter().any(|(k, _)| k == &id) {
            return Err("dupe");
        }
        let _ = self.items.push((id, handler));

        // TODO: Why does this throw lifetime errors?
        // self.items.sort_unstable_by_key(|(k, _)| k);
        Ok(())
    }

    /// Accessor function for the Context field
    pub fn context(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Attempt to dispatch the given message
    ///
    /// The bytes should consist of exactly one message (including the header).
    ///
    /// Returns an error in any of the following cases:
    ///
    /// * We failed to decode a header
    /// * No handler was found for the decoded key
    /// * The handler ran, but returned an error
    pub fn dispatch(&mut self, bytes: &[u8]) -> Result<(), Error<Err>> {
        let (hdr, remain) = extract_header_from_bytes(bytes)?;

        // TODO: switch to binary search once we sort?
        let Some(disp) = self
            .items
            .iter()
            .find_map(|(k, d)| if k == &hdr.key { Some(d) } else { None })
        else {
            return Err(Error::<Err>::NoMatchingHandler {
                key: hdr.key,
                seq_no: hdr.seq_no,
            });
        };
        (disp)(&hdr, &mut self.context, remain).map_err(Error::DispatchFailure)
    }
}

type Handler<C, E> = fn(&WireHeader, &mut C, &[u8]) -> Result<(), E>;

/// The WireHeader is appended to all messages
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct WireHeader {
    pub key: Key,
    pub seq_no: u32,
}

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
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize, Hash)]
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
    type Message: Schema;
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
    use postcard::experimental::schema::Schema;
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
        UnknownKey([u8; 8]),
        FailedToSpawn,
    }
}
