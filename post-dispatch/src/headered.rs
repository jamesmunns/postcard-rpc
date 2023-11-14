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


/// WARNING: This rehashes the schema! Prefer [to_slice_keyed]!
#[cfg(feature = "use-std")]
pub fn to_stdvec<T: Serialize + ?Sized + Schema>(
    seq_no: u32,
    path: &str,
    value: &T,
) -> Result<Vec<u8>, postcard::Error> {
    let flavor = Headered::try_new::<T>(postcard::ser_flavors::StdVec::new(), seq_no, path)?;
    postcard::serialize_with_flavor(value, flavor)
}

#[cfg(feature = "use-std")]
pub fn to_stdvec_keyed<T: Serialize + ?Sized + Schema>(
    seq_no: u32,
    key: Key,
    value: &T,
) -> Result<Vec<u8>, postcard::Error> {
    let flavor = Headered::try_new_keyed(postcard::ser_flavors::StdVec::new(), seq_no, key)?;
    postcard::serialize_with_flavor(value, flavor)
}

pub fn extract_header_from_bytes(slice: &[u8]) -> Result<(WireHeader, &[u8]), postcard::Error> {
    postcard::take_from_bytes::<WireHeader>(slice)
}
