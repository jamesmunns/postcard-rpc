#![no_std]

use blake2::{self, Blake2s, Digest};
use core::hash::{Hash, Hasher};
use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};

struct HashWrap {
    blake: Blake2s<blake2::digest::consts::U8>,
}

impl HashWrap {
    fn new() -> Self {
        Self {
            blake: Blake2s::new(),
        }
    }
}

impl Hasher for HashWrap {
    fn finish(&self) -> u64 {
        let mut out = Default::default();
        let blake = self.blake.clone();
        <Blake2s<blake2::digest::consts::U8> as Digest>::finalize_into(blake, &mut out);
        let out: [u8; 8] = out.into();
        u64::from_le_bytes(out)
    }

    fn write(&mut self, bytes: &[u8]) {
        self.blake.update(bytes);
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct Key([u8; 8]);

#[derive(Debug, PartialEq)]
pub enum Error<E> {
    NoMatchingHandler,
    DispatchFailure(E),
    Postcard(postcard::Error),
}

impl<E> From<postcard::Error> for Error<E> {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}

impl Key {
    pub fn for_path<T>(path: &str) -> Self
    where
        T: Schema,
    {
        let mut hasher = HashWrap::new();
        path.hash(&mut hasher);
        T::SCHEMA.hash(&mut hasher);
        Key(hasher.finish().to_le_bytes())
    }
}

type Handler<C, E> = fn(&mut C, &[u8]) -> Result<(), E>;

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

    pub fn dispatch(&mut self, bytes: &[u8]) -> Result<(), Error<E>> {
        let (key, remain) = postcard::take_from_bytes::<Key>(bytes)?;

        // TODO: switch to binary search once we sort?
        let Some(disp) = self
            .items
            .iter()
            .find_map(|(k, d)| if k == &key { Some(d) } else { None })
        else {
            return Err(Error::<E>::NoMatchingHandler);
        };
        (disp)(&mut self.context, remain).map_err(Error::DispatchFailure)
    }
}
