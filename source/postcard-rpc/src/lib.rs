//! Postcard RPC
//!
//! The goal of `postcard-rpc` is to make it easier for a
//! host PC to talk to a constrained device, like a microcontroller.
//!
//! See [the repo] for examples, and [the overview] for more details on how
//! to use this crate.
//!
//! [the repo]: https://github.com/jamesmunns/postcard-rpc
//! [the overview]: https://github.com/jamesmunns/postcard-rpc/blob/main/docs/overview.md

#![cfg_attr(not(any(test, feature = "use-std")), no_std)]

use blake2::{self, Blake2s, Digest};
use headered::extract_header_from_bytes;
use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};

pub mod accumulator;
pub mod hash;
pub mod headered;

#[cfg(feature = "use-std")]
pub mod host_client;

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
pub struct Dispatch<Context, Error, const N: usize> {
    items: heapless::Vec<(Key, Handler<Context, Error>), N>,
    context: Context,
}

impl<Context, E, const N: usize> Dispatch<Context, E, N> {
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
    pub fn add_handler<T: Schema>(
        &mut self,
        path: &str,
        handler: Handler<Context, E>,
    ) -> Result<(), &'static str> {
        if self.items.is_full() {
            return Err("full");
        }
        let id = Key::for_path::<T>(path);
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
    pub fn dispatch(&mut self, bytes: &[u8]) -> Result<(), Error<E>> {
        let (hdr, remain) = extract_header_from_bytes(bytes)?;

        // TODO: switch to binary search once we sort?
        let Some(disp) = self
            .items
            .iter()
            .find_map(|(k, d)| if k == &hdr.key { Some(d) } else { None })
        else {
            return Err(Error::<E>::NoMatchingHandler {
                key: hdr.key,
                seq_no: hdr.seq_no,
            });
        };
        (disp)(&hdr, &mut self.context, remain).map_err(Error::DispatchFailure)
    }
}

type Handler<C, E> = fn(&WireHeader, &mut C, &[u8]) -> Result<(), E>;

/// The WireHeader is appended to all messages
#[derive(Serialize, Deserialize, PartialEq)]
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
/// Specifically, we use [`Blake2s`](https://docs.rs/blake2/latest/blake2/),
/// and produce a 64-bit digest, by first hashing the path, then hashing the
/// schema. Blake2s is a cryptographic hash function, designed to be reasonably
/// efficient to compute even on small platforms like microcontrollers.
///
/// Changing **anything** about *either* of the path or the schema will produce
/// a drastically different `Key` value.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
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
    pub fn for_path<T>(path: &str) -> Self
    where
        T: Schema + ?Sized,
    {
        let mut hasher = hash::Hasher::new();
        hasher.update(path.as_bytes());
        hash::hash_schema::<T>(&mut hasher);
        let mut out = Default::default();
        <Blake2s<blake2::digest::consts::U8> as Digest>::finalize_into(hasher, &mut out);
        Key(out.into())
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
}

