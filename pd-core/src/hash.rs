use blake2::{self, Blake2s, Digest};
use postcard::experimental::schema::{Schema, NamedType, SdmTy, NamedVariant, NamedValue};

pub type Hasher = Blake2s<blake2::digest::consts::U8>;

pub fn hash_schema<T: Schema + ?Sized>(h: &mut Hasher) {
    let schema = T::SCHEMA;
    hash_named_type(h, schema);
}

fn hash_sdm_type(h: &mut Hasher, sdmty: &SdmTy) {
    match sdmty {
        SdmTy::Bool => h.update([0]),
        SdmTy::I8 => h.update([1]),
        SdmTy::U8 => h.update([2]),
        SdmTy::Varint(_) => h.update([3]),
        SdmTy::F32 => h.update([4]),
        SdmTy::F64 => h.update([5]),
        SdmTy::Char => h.update([6]),
        SdmTy::String => h.update([7]),
        SdmTy::ByteArray => h.update([8]),
        SdmTy::Option(nt) => {
            h.update([9]);
            hash_named_type(h, nt);
        },
        SdmTy::Unit => h.update([10]),
        SdmTy::UnitStruct => h.update([11]),
        SdmTy::UnitVariant => h.update([12]),
        SdmTy::NewtypeStruct(nt) => {
            h.update([13]);
            hash_named_type(h, nt);
        },
        SdmTy::NewtypeVariant(nt) => {
            h.update([14]);
            hash_named_type(h, nt);
        },
        SdmTy::Seq(nt) => {
            h.update([15]);
            hash_named_type(h, nt);
        },
        SdmTy::Tuple(nts) => {
            h.update([16]);
            for nt in nts.iter() {
                hash_named_type(h, nt);
            }
        },
        SdmTy::TupleStruct(nts) => {
            h.update([17]);
            for nt in nts.iter() {
                hash_named_type(h, nt);
            }
        },
        SdmTy::TupleVariant(nts) => {
            h.update([18]);
            for nt in nts.iter() {
                hash_named_type(h, nt);
            }
        },
        SdmTy::Map { key, val } => {
            h.update([19]);
            hash_named_type(h, key);
            hash_named_type(h, val);
        },
        SdmTy::Struct(nvs) => {
            h.update([20]);
            for nv in nvs.iter() {
                hash_named_value(h, nv)
            }
        },
        SdmTy::StructVariant(nvs) => {
            h.update([21]);
            for nv in nvs.iter() {
                hash_named_value(h, nv)
            }
        },
        SdmTy::Enum(nvs) => {
            h.update([22]);
            for nv in nvs.iter() {
                hash_named_variant(h, nv)
            }
        },
    }
}

fn hash_named_type(h: &mut Hasher, nt: &NamedType) {
    h.update(nt.name.as_bytes());
    hash_sdm_type(h, nt.ty);
}

fn hash_named_variant(h: &mut Hasher, nt: &NamedVariant) {
    h.update(nt.name.as_bytes());
    hash_sdm_type(h, nt.ty);
}

fn hash_named_value(h: &mut Hasher, nt: &NamedValue) {
    h.update(nt.name.as_bytes());
    hash_named_type(h, nt.ty);
}
