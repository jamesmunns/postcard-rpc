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
        Self {
            state: Self::BASIS
        }
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
