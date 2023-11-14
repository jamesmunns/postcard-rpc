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

#[derive(Serialize, Deserialize, PartialEq)]
pub struct WireHeader {
    pub key: Key,
    pub seq_no: u32,
}

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

#[derive(Debug, PartialEq)]
pub enum Error<E> {
    NoMatchingHandler { key: Key, seq_no: u32 },
    DispatchFailure(E),
    Postcard(postcard::Error),
}

impl<E> From<postcard::Error> for Error<E> {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}

impl Key {
    pub fn for_path<T: ?Sized>(path: &str) -> Self
    where
        T: Schema,
    {
        let mut hasher = hash::Hasher::new();
        hasher.update(path.as_bytes());
        hash::hash_schema::<T>(&mut hasher);
        let mut out = Default::default();
        <Blake2s<blake2::digest::consts::U8> as Digest>::finalize_into(hasher, &mut out);
        Key(out.into())
    }
}

type Handler<C, E> = fn(&WireHeader, &mut C, &[u8]) -> Result<(), E>;

pub struct Dispatch<Context, Error, const N: usize> {
    items: heapless::Vec<(Key, Handler<Context, Error>), N>,
    context: Context,
}

impl<Context, E, const N: usize> Dispatch<Context, E, N> {
    pub fn new(c: Context) -> Self {
        Self {
            items: heapless::Vec::new(),
            context: c,
        }
    }

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

    pub fn context(&mut self) -> &mut Context {
        &mut self.context
    }

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
