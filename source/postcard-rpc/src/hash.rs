//! URI and Schema Hashing
//!
//! We use `FNV1a` hashes with a digest size of 64 bits to represent dispatch keys.
//!
//! Unfortunately. using [core::hash::Hash] seems to not produce consistent results,
//! which [was noted] in the docs. To overcome this, we implement a custom method for
//! hashing the postcard [Schema].
//!
//! [was noted]: https://doc.rust-lang.org/stable/std/hash/trait.Hash.html#portability

use postcard::experimental::schema::{NamedType, NamedValue, NamedVariant, Schema, SdmTy, Varint};

pub struct Fnv1a64Hasher {
    state: u64,
}

impl Fnv1a64Hasher {
    // source: https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
    const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub fn new() -> Self {
        Self { state: Self::BASIS }
    }

    pub fn update(&mut self, data: &[u8]) {
        for b in data {
            let ext = u64::from(*b);
            self.state ^= ext;
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }

    pub fn digest(self) -> u64 {
        self.state
    }

    pub fn digest_bytes(self) -> [u8; 8] {
        self.digest().to_le_bytes()
    }
}

impl Default for Fnv1a64Hasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "hashv2"))]
pub mod fnv1a64 {
    use super::*;

    pub const fn hash_ty_path<T: Schema + ?Sized>(path: &str) -> [u8; 8] {
        let schema = T::SCHEMA;
        let state = hash_update_str(Fnv1a64Hasher::BASIS, path);
        hash_named_type(state, schema).to_le_bytes()
    }

    const fn hash_update(mut state: u64, bytes: &[u8]) -> u64 {
        let mut idx = 0;
        while idx < bytes.len() {
            let ext = bytes[idx] as u64;
            state ^= ext;
            state = state.wrapping_mul(Fnv1a64Hasher::PRIME);
            idx += 1;
        }
        state
    }

    const fn hash_update_str(state: u64, s: &str) -> u64 {
        hash_update(state, s.as_bytes())
    }

    const fn hash_sdm_type(state: u64, sdmty: &'static SdmTy) -> u64 {
        // The actual values we use here don't matter that much (as far as I know),
        // as long as the values for each variant are unique. I am unsure of the
        // implications of doing a TON of single byte calls to `update`, it may be
        // worth doing some buffering, and only calling update every 4/8/16 bytes
        // instead, if performance is a concern.
        //
        // As of initial implementation, I'm mostly concerned with "does it work",
        // as hashing is typically only done on startup.
        match sdmty {
            SdmTy::Bool => hash_update(state, &[0]),
            SdmTy::I8 => hash_update(state, &[1]),
            SdmTy::U8 => hash_update(state, &[2]),
            SdmTy::Varint(v) => {
                let state = hash_update(state, &[3]);
                match v {
                    Varint::I16 => hash_update(state, &[0]),
                    Varint::I32 => hash_update(state, &[1]),
                    Varint::I64 => hash_update(state, &[2]),
                    Varint::I128 => hash_update(state, &[3]),
                    Varint::U16 => hash_update(state, &[4]),
                    Varint::U32 => hash_update(state, &[5]),
                    Varint::U64 => hash_update(state, &[6]),
                    Varint::U128 => hash_update(state, &[7]),
                    Varint::Usize => hash_update(state, &[8]),
                    Varint::Isize => hash_update(state, &[9]),
                }
            }
            SdmTy::F32 => hash_update(state, &[4]),
            SdmTy::F64 => hash_update(state, &[5]),
            SdmTy::Char => hash_update(state, &[6]),
            SdmTy::String => hash_update(state, &[7]),
            SdmTy::ByteArray => hash_update(state, &[8]),
            SdmTy::Option(nt) => {
                let state = hash_update(state, &[9]);
                hash_named_type(state, nt)
            }
            SdmTy::Unit => hash_update(state, &[10]),
            SdmTy::UnitStruct => hash_update(state, &[11]),
            SdmTy::UnitVariant => hash_update(state, &[12]),
            SdmTy::NewtypeStruct(nt) => {
                let state = hash_update(state, &[13]);
                hash_named_type(state, nt)
            }
            SdmTy::NewtypeVariant(nt) => {
                let state = hash_update(state, &[14]);
                hash_named_type(state, nt)
            }
            SdmTy::Seq(nt) => {
                let state = hash_update(state, &[15]);
                hash_named_type(state, nt)
            }
            SdmTy::Tuple(nts) => {
                let mut state = hash_update(state, &[16]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type(state, nts[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::TupleStruct(nts) => {
                let mut state = hash_update(state, &[17]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type(state, nts[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::TupleVariant(nts) => {
                let mut state = hash_update(state, &[18]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type(state, nts[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::Map { key, val } => {
                let state = hash_update(state, &[19]);
                let state = hash_named_type(state, key);
                hash_named_type(state, val)
            }
            SdmTy::Struct(nvs) => {
                let mut state = hash_update(state, &[20]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_value(state, nvs[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::StructVariant(nvs) => {
                let mut state = hash_update(state, &[21]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_value(state, nvs[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::Enum(nvs) => {
                let mut state = hash_update(state, &[22]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_variant(state, nvs[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::Schema => hash_update(state, &[23]),
        }
    }

    const fn hash_named_type(state: u64, nt: &NamedType) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_sdm_type(state, nt.ty)
    }

    const fn hash_named_variant(state: u64, nt: &NamedVariant) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_sdm_type(state, nt.ty)
    }

    const fn hash_named_value(state: u64, nt: &NamedValue) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_named_type(state, nt.ty)
    }
}

#[cfg(feature = "hashv2")]
pub mod fnv1a64 {
    use super::*;

    pub const fn hash_ty_path<T: Schema + ?Sized>(path: &str) -> [u8; 8] {
        let schema = T::SCHEMA;
        let state = hash_update_str(Fnv1a64Hasher::BASIS, path);
        hash_named_type(state, schema).to_le_bytes()
    }

    pub(crate) const fn hash_update(mut state: u64, bytes: &[u8]) -> u64 {
        let mut idx = 0;
        while idx < bytes.len() {
            let ext = bytes[idx] as u64;
            state ^= ext;
            state = state.wrapping_mul(Fnv1a64Hasher::PRIME);
            idx += 1;
        }
        state
    }

    pub(crate) const fn hash_update_str(state: u64, s: &str) -> u64 {
        hash_update(state, s.as_bytes())
    }

    const fn hash_sdm_type(state: u64, sdmty: &'static SdmTy) -> u64 {
        // The actual values we use here don't matter that much (as far as I know),
        // as long as the values for each variant are unique. I am unsure of the
        // implications of doing a TON of single byte calls to `update`, it may be
        // worth doing some buffering, and only calling update every 4/8/16 bytes
        // instead, if performance is a concern.
        //
        // As of initial implementation, I'm mostly concerned with "does it work",
        // as hashing is typically only done on startup.
        //
        // Using all primes that fit into a single byte:
        //
        // all_primes = [
        //     0x02, 0x03, 0x05, 0x07, 0x0B, 0x0D, 0x11, 0x13,
        //     0x17, 0x1D, 0x1F, 0x25, 0x29, 0x2B, 0x2F, 0x35,
        //     0x3B, 0x3D, 0x43, 0x47, 0x49, 0x4F, 0x53, 0x59,
        //     0x61, 0x65, 0x67, 0x6B, 0x6D, 0x71, 0x7F, 0x83,
        //     0x89, 0x8B, 0x95, 0x97, 0x9D, 0xA3, 0xA7, 0xAD,
        //     0xB3, 0xB5, 0xBF, 0xC1, 0xC5, 0xC7, 0xD3, 0xDF,
        //     0xE3, 0xE5, 0xE9, 0xEF, 0xF1, 0xFB,
        // ];
        // shuffled_primes = [
        //     0x11, 0xC5, 0x3D, 0x95, 0x1D, 0x0D, 0x0B, 0x02,
        //     0x83, 0xD3, 0x13, 0x8B, 0x6B, 0xAD, 0xEF, 0x71,
        //     0xC1, 0x25, 0x65, 0x6D, 0x47, 0xBF, 0xB5, 0x9D,
        //     0xDF, 0x03, 0xA7, 0x05, 0xC7, 0x4F, 0x7F, 0x67,
        //     0xE9, 0xB3, 0xE5, 0x2B, 0x97, 0xFB, 0x61, 0x3B,
        //     0x1F, 0xA3, 0x35, 0x43, 0x89, 0x49, 0xE3, 0x07,
        //     0x53, 0xF1, 0x17, 0x2F, 0x29, 0x59,
        // ];
        match sdmty {
            SdmTy::Bool => hash_update(state, &[0x11]),
            SdmTy::I8 => hash_update(state, &[0xC5]),
            SdmTy::U8 => hash_update(state, &[0x3D]),
            SdmTy::Varint(v) => {
                let state = hash_update(state, &[0x95]);
                match v {
                    Varint::I16 => hash_update(state, &[0x1D]),
                    Varint::I32 => hash_update(state, &[0x0D]),
                    Varint::I64 => hash_update(state, &[0x0B]),
                    Varint::I128 => hash_update(state, &[0x02]),
                    Varint::U16 => hash_update(state, &[0x83]),
                    Varint::U32 => hash_update(state, &[0xD3]),
                    Varint::U64 => hash_update(state, &[0x13]),
                    Varint::U128 => hash_update(state, &[0x8B]),
                    Varint::Usize => hash_update(state, &[0x6B]),
                    Varint::Isize => hash_update(state, &[0xAD]),
                }
            }
            SdmTy::F32 => hash_update(state, &[0xEF]),
            SdmTy::F64 => hash_update(state, &[0x71]),
            SdmTy::Char => hash_update(state, &[0xC1]),
            SdmTy::String => hash_update(state, &[0x25]),
            SdmTy::ByteArray => hash_update(state, &[0x65]),
            SdmTy::Option(nt) => {
                let state = hash_update(state, &[0x6D]);
                hash_named_type(state, nt)
            }
            SdmTy::Unit => hash_update(state, &[0x47]),
            SdmTy::UnitStruct => hash_update(state, &[0xBF]),
            SdmTy::UnitVariant => hash_update(state, &[0xB5]),
            SdmTy::NewtypeStruct(nt) => {
                let state = hash_update(state, &[0x9D]);
                hash_named_type(state, nt)
            }
            SdmTy::NewtypeVariant(nt) => {
                let state = hash_update(state, &[0xDF]);
                hash_named_type(state, nt)
            }
            SdmTy::Seq(nt) => {
                let state = hash_update(state, &[0x03]);
                hash_named_type(state, nt)
            }
            SdmTy::Tuple(nts) => {
                let mut state = hash_update(state, &[0xA7]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type(state, nts[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::TupleStruct(nts) => {
                let mut state = hash_update(state, &[0x05]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type(state, nts[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::TupleVariant(nts) => {
                let mut state = hash_update(state, &[0xC7]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type(state, nts[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::Map { key, val } => {
                let state = hash_update(state, &[0x4F]);
                let state = hash_named_type(state, key);
                hash_named_type(state, val)
            }
            SdmTy::Struct(nvs) => {
                let mut state = hash_update(state, &[0x7F]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_value(state, nvs[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::StructVariant(nvs) => {
                let mut state = hash_update(state, &[0x67]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_value(state, nvs[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::Enum(nvs) => {
                let mut state = hash_update(state, &[0xE9]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_variant(state, nvs[idx]);
                    idx += 1;
                }
                state
            }
            SdmTy::Schema => hash_update(state, &[0xB3]),
        }
    }

    const fn hash_named_type(state: u64, nt: &NamedType) -> u64 {
        // NOTE: We do *not* hash the name of the type in hashv2. This
        // is to allow "safe" type punning, e.g. treating `Vec<u8>` and
        // `&[u8]` as compatible types, when talking between std and no-std
        // targets
        //
        // let state = hash_update(state, nt.name.as_bytes());
        hash_sdm_type(state, nt.ty)
    }

    const fn hash_named_variant(state: u64, nt: &NamedVariant) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_sdm_type(state, nt.ty)
    }

    const fn hash_named_value(state: u64, nt: &NamedValue) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_named_type(state, nt.ty)
    }
}

#[cfg(all(feature = "hashv2", feature = "use-std"))]
pub mod fnv1a64_owned {
    use postcard::experimental::schema::{
        OwnedNamedType, OwnedNamedValue, OwnedNamedVariant, OwnedSdmTy,
    };

    use super::fnv1a64::*;
    use super::*;

    pub fn hash_ty_path_owned(path: &str, nt: &OwnedNamedType) -> [u8; 8] {
        let state = hash_update_str(Fnv1a64Hasher::BASIS, path);
        hash_named_type_owned(state, nt).to_le_bytes()
    }

    fn hash_sdm_type_owned(state: u64, sdmty: &OwnedSdmTy) -> u64 {
        // The actual values we use here don't matter that much (as far as I know),
        // as long as the values for each variant are unique. I am unsure of the
        // implications of doing a TON of single byte calls to `update`, it may be
        // worth doing some buffering, and only calling update every 4/8/16 bytes
        // instead, if performance is a concern.
        //
        // As of initial implementation, I'm mostly concerned with "does it work",
        // as hashing is typically only done on startup.
        //
        // Using all primes that fit into a single byte:
        //
        // all_primes = [
        //     0x02, 0x03, 0x05, 0x07, 0x0B, 0x0D, 0x11, 0x13,
        //     0x17, 0x1D, 0x1F, 0x25, 0x29, 0x2B, 0x2F, 0x35,
        //     0x3B, 0x3D, 0x43, 0x47, 0x49, 0x4F, 0x53, 0x59,
        //     0x61, 0x65, 0x67, 0x6B, 0x6D, 0x71, 0x7F, 0x83,
        //     0x89, 0x8B, 0x95, 0x97, 0x9D, 0xA3, 0xA7, 0xAD,
        //     0xB3, 0xB5, 0xBF, 0xC1, 0xC5, 0xC7, 0xD3, 0xDF,
        //     0xE3, 0xE5, 0xE9, 0xEF, 0xF1, 0xFB,
        // ];
        // shuffled_primes = [
        //     0x11, 0xC5, 0x3D, 0x95, 0x1D, 0x0D, 0x0B, 0x02,
        //     0x83, 0xD3, 0x13, 0x8B, 0x6B, 0xAD, 0xEF, 0x71,
        //     0xC1, 0x25, 0x65, 0x6D, 0x47, 0xBF, 0xB5, 0x9D,
        //     0xDF, 0x03, 0xA7, 0x05, 0xC7, 0x4F, 0x7F, 0x67,
        //     0xE9, 0xB3, 0xE5, 0x2B, 0x97, 0xFB, 0x61, 0x3B,
        //     0x1F, 0xA3, 0x35, 0x43, 0x89, 0x49, 0xE3, 0x07,
        //     0x53, 0xF1, 0x17, 0x2F, 0x29, 0x59,
        // ];
        match sdmty {
            OwnedSdmTy::Bool => hash_update(state, &[0x11]),
            OwnedSdmTy::I8 => hash_update(state, &[0xC5]),
            OwnedSdmTy::U8 => hash_update(state, &[0x3D]),
            OwnedSdmTy::Varint(v) => {
                let state = hash_update(state, &[0x95]);
                match v {
                    Varint::I16 => hash_update(state, &[0x1D]),
                    Varint::I32 => hash_update(state, &[0x0D]),
                    Varint::I64 => hash_update(state, &[0x0B]),
                    Varint::I128 => hash_update(state, &[0x02]),
                    Varint::U16 => hash_update(state, &[0x83]),
                    Varint::U32 => hash_update(state, &[0xD3]),
                    Varint::U64 => hash_update(state, &[0x13]),
                    Varint::U128 => hash_update(state, &[0x8B]),
                    Varint::Usize => hash_update(state, &[0x6B]),
                    Varint::Isize => hash_update(state, &[0xAD]),
                }
            }
            OwnedSdmTy::F32 => hash_update(state, &[0xEF]),
            OwnedSdmTy::F64 => hash_update(state, &[0x71]),
            OwnedSdmTy::Char => hash_update(state, &[0xC1]),
            OwnedSdmTy::String => hash_update(state, &[0x25]),
            OwnedSdmTy::ByteArray => hash_update(state, &[0x65]),
            OwnedSdmTy::Option(nt) => {
                let state = hash_update(state, &[0x6D]);
                hash_named_type_owned(state, nt)
            }
            OwnedSdmTy::Unit => hash_update(state, &[0x47]),
            OwnedSdmTy::UnitStruct => hash_update(state, &[0xBF]),
            OwnedSdmTy::UnitVariant => hash_update(state, &[0xB5]),
            OwnedSdmTy::NewtypeStruct(nt) => {
                let state = hash_update(state, &[0x9D]);
                hash_named_type_owned(state, nt)
            }
            OwnedSdmTy::NewtypeVariant(nt) => {
                let state = hash_update(state, &[0xDF]);
                hash_named_type_owned(state, nt)
            }
            OwnedSdmTy::Seq(nt) => {
                let state = hash_update(state, &[0x03]);
                hash_named_type_owned(state, nt)
            }
            OwnedSdmTy::Tuple(nts) => {
                let mut state = hash_update(state, &[0xA7]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type_owned(state, &nts[idx]);
                    idx += 1;
                }
                state
            }
            OwnedSdmTy::TupleStruct(nts) => {
                let mut state = hash_update(state, &[0x05]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type_owned(state, &nts[idx]);
                    idx += 1;
                }
                state
            }
            OwnedSdmTy::TupleVariant(nts) => {
                let mut state = hash_update(state, &[0xC7]);
                let mut idx = 0;
                while idx < nts.len() {
                    state = hash_named_type_owned(state, &nts[idx]);
                    idx += 1;
                }
                state
            }
            OwnedSdmTy::Map { key, val } => {
                let state = hash_update(state, &[0x4F]);
                let state = hash_named_type_owned(state, key);
                hash_named_type_owned(state, val)
            }
            OwnedSdmTy::Struct(nvs) => {
                let mut state = hash_update(state, &[0x7F]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_value_owned(state, &nvs[idx]);
                    idx += 1;
                }
                state
            }
            OwnedSdmTy::StructVariant(nvs) => {
                let mut state = hash_update(state, &[0x67]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_value_owned(state, &nvs[idx]);
                    idx += 1;
                }
                state
            }
            OwnedSdmTy::Enum(nvs) => {
                let mut state = hash_update(state, &[0xE9]);
                let mut idx = 0;
                while idx < nvs.len() {
                    state = hash_named_variant_owned(state, &nvs[idx]);
                    idx += 1;
                }
                state
            }
            OwnedSdmTy::Schema => hash_update(state, &[0xB3]),
        }
    }

    fn hash_named_type_owned(state: u64, nt: &OwnedNamedType) -> u64 {
        // NOTE: We do *not* hash the name of the type in hashv2. This
        // is to allow "safe" type punning, e.g. treating `Vec<u8>` and
        // `&[u8]` as compatible types, when talking between std and no-std
        // targets
        //
        // let state = hash_update(state, nt.name.as_bytes());
        hash_sdm_type_owned(state, &nt.ty)
    }

    fn hash_named_variant_owned(state: u64, nt: &OwnedNamedVariant) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_sdm_type_owned(state, &nt.ty)
    }

    fn hash_named_value_owned(state: u64, nt: &OwnedNamedValue) -> u64 {
        let state = hash_update(state, nt.name.as_bytes());
        hash_named_type_owned(state, &nt.ty)
    }
}

#[cfg(all(test, feature = "hashv2"))]
mod test {
    use super::fnv1a64::hash_ty_path;
    use super::*;

    #[test]
    fn type_punning_good() {
        let hash_1 = hash_ty_path::<Vec<u8>>("test_path");
        let hash_2 = hash_ty_path::<&[u8]>("test_path");
        let hash_3 = hash_ty_path::<Vec<u16>>("test_path");
        let hash_4 = hash_ty_path::<&[u16]>("test_path");
        let hash_5 = hash_ty_path::<Vec<u8>>("test_patt");
        let hash_6 = hash_ty_path::<&[u8]>("test_patt");
        assert_eq!(hash_1, hash_2);
        assert_eq!(hash_3, hash_4);
        assert_ne!(hash_1, hash_3);
        assert_ne!(hash_2, hash_4);
        assert_ne!(hash_1, hash_5);
        assert_ne!(hash_2, hash_6);
    }

    // TODO: It is questionable if I like this outcome
    #[test]
    fn type_punning_questionable() {
        #[derive(Schema)]
        #[allow(unused)]
        struct Wrapper1(u8);

        #[derive(Schema)]
        #[allow(unused)]
        struct Wrapper2(u8);

        let hash_1 = hash_ty_path::<Wrapper1>("test_path");
        let hash_2 = hash_ty_path::<Wrapper2>("test_path");
        assert_eq!(hash_1, hash_2);
    }
}
