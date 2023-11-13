#![no_std]

use blake2::{self, Blake2s, Digest};
use core::hash::{Hash, Hasher};
use headered::extract_header_from_bytes;
use postcard::experimental::schema::Schema;
use serde::{Deserialize, Serialize};

pub mod accumulator;
pub mod hash;

pub mod headered {
    use crate::{Key, WireHeader};
    use postcard::{
        experimental::schema::Schema,
        ser_flavors::{Cobs, Flavor as SerFlavor, Slice},
        Serializer,
    };
    use serde::Serialize;

    struct Headered<B: SerFlavor> {
        flavor: B,
    }

    impl<B: SerFlavor> Headered<B> {
        fn try_new_keyed(b: B, seq_no: u32, key: Key) -> Result<Self, postcard::Error> {
            let mut serializer = Serializer { output: b };
            let hdr = WireHeader { key, seq_no };
            hdr.serialize(&mut serializer)?;
            Ok(Self {
                flavor: serializer.output,
            })
        }

        fn try_new<T: Schema + ?Sized>(
            b: B,
            seq_no: u32,
            path: &str,
        ) -> Result<Self, postcard::Error> {
            let key = Key::for_path::<T>(path);
            Self::try_new_keyed(b, seq_no, key)
        }
    }

    impl<B: SerFlavor> SerFlavor for Headered<B> {
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

    /// WARNING: This rehashes the schema! Prefer [to_slice_keyed]!
    pub fn to_slice<'a, T: Serialize + ?Sized + Schema>(
        seq_no: u32,
        path: &str,
        value: &T,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], postcard::Error> {
        let flavor = Headered::try_new::<T>(Slice::new(buf), seq_no, path)?;
        postcard::serialize_with_flavor(value, flavor)
    }

    pub fn to_slice_keyed<'a, T: Serialize + ?Sized + Schema>(
        seq_no: u32,
        key: Key,
        value: &T,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], postcard::Error> {
        let flavor = Headered::try_new_keyed(Slice::new(buf), seq_no, key)?;
        postcard::serialize_with_flavor(value, flavor)
    }

    /// WARNING: This rehashes the schema! Prefer [to_slice_cobs_keyed]!
    pub fn to_slice_cobs<'a, T: Serialize + ?Sized + Schema>(
        seq_no: u32,
        path: &str,
        value: &T,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], postcard::Error> {
        let flavor = Headered::try_new::<T>(Cobs::try_new(Slice::new(buf))?, seq_no, path)?;
        postcard::serialize_with_flavor(value, flavor)
    }

    pub fn to_slice_cobs_keyed<'a, T: Serialize + ?Sized + Schema>(
        seq_no: u32,
        key: Key,
        value: &T,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], postcard::Error> {
        let flavor = Headered::try_new_keyed(Cobs::try_new(Slice::new(buf))?, seq_no, key)?;
        postcard::serialize_with_flavor(value, flavor)
    }

    pub fn extract_header_from_bytes(slice: &[u8]) -> Result<(WireHeader, &[u8]), postcard::Error> {
        postcard::take_from_bytes::<WireHeader>(slice)
    }
}

#[derive(Serialize, Deserialize)]
pub struct WireHeader {
    pub key: Key,
    pub seq_no: u32,
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Serialize, Deserialize)]
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
        #[cfg(feature = "defmt")]
        defmt::info!("adding {:?} '{}'", id, path);
        if self.items.iter().any(|(k, _)| k == &id) {
            return Err("dupe");
        }
        let _ = self.items.push((id, handler));

        // TODO: Why does this throw lifetime errors?
        // self.items.sort_unstable_by_key(|(k, _)| k);
        Ok(())
    }

    pub fn dispatch(&mut self, bytes: &[u8]) -> Result<(), Error<E>> {
        let (hdr, remain) = extract_header_from_bytes(bytes)?;

        // TODO: switch to binary search once we sort?
        let Some(disp) = self
            .items
            .iter()
            .find_map(|(k, d)| if k == &hdr.key { Some(d) } else { None })
        else {
            return Err(Error::<E>::NoMatchingHandler);
        };
        (disp)(&hdr, &mut self.context, remain).map_err(Error::DispatchFailure)
    }
}
