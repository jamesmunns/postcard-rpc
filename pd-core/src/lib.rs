#![no_std]

use blake2::{self, Blake2s, Digest};
use core::hash::{Hash, Hasher};
use headered::extract_header_from_bytes;
use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};

pub mod headered {
    use crate::Key;
    use postcard::{
        experimental::schema::Schema,
        ser_flavors::{Cobs, Flavor as SerFlavor, Slice},
    };
    use serde::Serialize;

    struct Header<B: SerFlavor> {
        flavor: B,
    }

    impl<B: SerFlavor> Header<B> {
        fn try_new<T: Schema + ?Sized>(mut b: B, path: &str) -> Result<Self, postcard::Error> {
            let key_bytes = Key::for_path::<T>(path).0;
            b.try_extend(&key_bytes)?;
            Ok(Self { flavor: b })
        }
    }

    impl<B: SerFlavor> SerFlavor for Header<B> {
        type Output = B::Output;

        #[inline]
        fn try_push(&mut self, data: u8) -> postcard::Result<()> {
            self.flavor.try_push(data)
        }

        #[inline]
        fn finalize(self) -> postcard::Result<Self::Output> {
            self.flavor.finalize()
        }

        #[inline]
        fn try_extend(&mut self, data: &[u8]) -> postcard::Result<()> {
            self.flavor.try_extend(data)
        }
    }

    pub fn to_slice<'a, T: Serialize + ?Sized + Schema>(
        path: &str,
        value: &T,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], postcard::Error> {
        let flavor = Header::try_new::<T>(Slice::new(buf), path)?;
        postcard::serialize_with_flavor(value, flavor)
    }

    pub fn to_slice_cobs<'a, T: Serialize + ?Sized + Schema>(
        path: &str,
        value: &T,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], postcard::Error> {
        let flavor = Header::try_new::<T>(Cobs::try_new(Slice::new(buf))?, path)?;
        postcard::serialize_with_flavor(value, flavor)
    }

    pub fn extract_header_from_bytes(slice: &[u8]) -> Result<(Key, &[u8]), postcard::Error> {
        if slice.len() < 8 {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        let (key, body) = slice.split_at(8);
        let mut key_bytes = [0u8; 8];
        key_bytes.copy_from_slice(key);
        Ok((Key(key_bytes), body))
    }

    pub fn extract_header_from_bytes_cobs(
        slice: &mut [u8],
    ) -> Result<(Key, &[u8]), postcard::Error> {
        let used =
            cobs::decode_in_place(slice).map_err(|_| postcard::Error::DeserializeBadEncoding)?;
        extract_header_from_bytes(&slice[..used])
    }
}

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
    pub fn for_path<T: ?Sized>(path: &str) -> Self
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
        let (key, remain) = extract_header_from_bytes(bytes)?;

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
