//! Create unique type lists at compile time
//!
//! This is an excercise in the capabilities of macros and const fns.
//!
//! From a very high level, the process goes like this:
//!
//! 1. We recursively look at a type, counting how many types it contains,
//!      WITHOUT considering de-duplication. This is used as an "upper bound"
//!      of the number of potential types we could have to report
//! 2. Create an array of `[Option<&DataModelType>; MAX]` that we use something
//!      like an append-only vec.
//! 3. Recursively traverse the type AGAIN, this time collecting all unique
//!      non-primitive types we encounter, and adding them to the list. This
//!      is outrageously inefficient, but it is done at const time with all
//!      the restrictions it entails, because we don't pay at runtime.
//! 4. Record how many types we ACTUALLY collected in step 3, and create a
//!      new array, `[&DataModelType; ACTUAL]`, and copy the unique types into
//!      this new array
//! 5. Convert this `[&DataModelType; N]` array into a `&'static [&DataModelType]`
//!      array to make it possible to handle with multiple types
//! 6. If we are collecting MULTIPLE types into a single aggregate report,
//!      then we make a new array of `[Option<&DataModelType>; sum(all types)]`,
//!      by calculating the sum of types contained for each list calculated
//!      in step 4.
//! 7. We then perform the same "merging" process from 3, pushing any unique
//!      type we find into the aggregate list, and recording the number of
//!      unique types we found in the entire set.
//! 8. We then perform the same "shrinking" process from step 4, leaving us
//!      with a single array, `[&DataModelType; TOTAL]` containing all unique types
//! 9. We then perform the same "slicing" process from step 5, to get our
//!      final `&'static [&DataModelType]`.

use postcard_schema_ng::{
    schema::{Data, DataModelType, NamedField, Variant},
    Schema,
};

//////////////////////////////////////////////////////////////////////////////
// STAGE 0 - HELPERS
//////////////////////////////////////////////////////////////////////////////

/// `is_prim` returns whether the type is a *primitive*, or a built-in type that
/// does not need to be sent over the wire.
const fn is_prim(dmt: &DataModelType) -> bool {
    match dmt {
        // These are all primitives
        DataModelType::Bool => true,
        DataModelType::I8 => true,
        DataModelType::U8 => true,
        DataModelType::I16 => true,
        DataModelType::I32 => true,
        DataModelType::I64 => true,
        DataModelType::I128 => true,
        DataModelType::U16 => true,
        DataModelType::U32 => true,
        DataModelType::U64 => true,
        DataModelType::U128 => true,
        DataModelType::Usize => true,
        DataModelType::Isize => true,
        DataModelType::F32 => true,
        DataModelType::F64 => true,
        DataModelType::Char => true,
        DataModelType::String => true,
        DataModelType::ByteArray => true,
        DataModelType::Unit => true,
        DataModelType::Schema => true,

        // Items with subtypes are composite, and therefore not primitives, as
        // we need to convey this information.
        DataModelType::Option(_) | DataModelType::Seq(_) => false,
        DataModelType::Tuple(_) => false,
        DataModelType::Map { .. } => false,
        DataModelType::Struct { .. } => false,
        DataModelType::Enum { .. } => false,
    }
}

/// A const version of `<str as PartialEq>::eq`
const fn str_eq(a: &str, b: &str) -> bool {
    let mut i = 0;
    if a.len() != b.len() {
        return false;
    }
    let a_by = a.as_bytes();
    let b_by = b.as_bytes();
    while i < a.len() {
        if a_by[i] != b_by[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn data_eq(a: &Data, b: &Data) -> bool {
    match (a, b) {
        (Data::Unit, Data::Unit) => true,
        (Data::Newtype(dmta), Data::Newtype(dmtb)) => dmt_eq(dmta, dmtb),
        (Data::Tuple(dmtas), Data::Tuple(dmtbs)) => dmts_eq(dmtas, dmtbs),
        (Data::Struct(nfas), Data::Struct(nfbs)) => nfs_eq(nfas, nfbs),
        _ => false,
    }
}

/// A const version of `<[&DataModelType] as PartialEq>::eq`
const fn dmts_eq(a: &[&DataModelType], b: &[&DataModelType]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if !dmt_eq(a[i], b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// A const version of `<DataModelType as PartialEq>::eq`
const fn dmt_eq(a: &DataModelType, b: &DataModelType) -> bool {
    match (a, b) {
        // Data model types are ONLY matching if they are both the same variant
        //
        // For primitives (and unit structs), we only check the discriminant matches.
        (DataModelType::Bool, DataModelType::Bool) => true,
        (DataModelType::I8, DataModelType::I8) => true,
        (DataModelType::U8, DataModelType::U8) => true,
        (DataModelType::I16, DataModelType::I16) => true,
        (DataModelType::I32, DataModelType::I32) => true,
        (DataModelType::I64, DataModelType::I64) => true,
        (DataModelType::I128, DataModelType::I128) => true,
        (DataModelType::U16, DataModelType::U16) => true,
        (DataModelType::U32, DataModelType::U32) => true,
        (DataModelType::U64, DataModelType::U64) => true,
        (DataModelType::U128, DataModelType::U128) => true,
        (DataModelType::Usize, DataModelType::Usize) => true,
        (DataModelType::Isize, DataModelType::Isize) => true,
        (DataModelType::F32, DataModelType::F32) => true,
        (DataModelType::F64, DataModelType::F64) => true,
        (DataModelType::Char, DataModelType::Char) => true,
        (DataModelType::String, DataModelType::String) => true,
        (DataModelType::ByteArray, DataModelType::ByteArray) => true,
        (DataModelType::Unit, DataModelType::Unit) => true,
        (DataModelType::Schema, DataModelType::Schema) => true,

        // For non-primitive types, we check whether all children are equivalent as well.
        (DataModelType::Option(dmta), DataModelType::Option(dmtb)) => dmt_eq(dmta, dmtb),
        (DataModelType::Seq(dmta), DataModelType::Seq(dmtb)) => dmt_eq(dmta, dmtb),

        (DataModelType::Tuple(dmtsa), DataModelType::Tuple(dmtsb)) => dmts_eq(dmtsa, dmtsb),
        (
            DataModelType::Map {
                key: keya,
                val: vala,
            },
            DataModelType::Map {
                key: keyb,
                val: valb,
            },
        ) => dmt_eq(keya, keyb) && dmt_eq(vala, valb),
        (
            DataModelType::Struct { name: na, data: da },
            DataModelType::Struct { name: nb, data: db },
        ) => str_eq(na, nb) && data_eq(da, db),
        (
            DataModelType::Enum {
                name: na,
                variants: vas,
            },
            DataModelType::Enum {
                name: nb,
                variants: vbs,
            },
        ) => str_eq(na, nb) && vars_eq(vas, vbs),

        // Any mismatches are not equal
        _ => false,
    }
}

/// A const version of `<Variant as PartialEq>::eq`
const fn var_eq(a: &Variant, b: &Variant) -> bool {
    str_eq(a.name, b.name) && data_eq(&a.data, &b.data)
}

/// A const version of `<&[&Variant] as PartialEq>::eq`
const fn vars_eq(a: &[&Variant], b: &[&Variant]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if !var_eq(a[i], b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

const fn nf_eq(a: &NamedField, b: &NamedField) -> bool {
    str_eq(a.name, b.name) && dmt_eq(a.ty, b.ty)
}

const fn nfs_eq(a: &[&NamedField], b: &[&NamedField]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if !nf_eq(a[i], b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

//////////////////////////////////////////////////////////////////////////////
// STAGE 1 - UPPER BOUND CALCULATION
//////////////////////////////////////////////////////////////////////////////

/// Count the number of unique types contained by this DataModelType,
/// ONLY counting children, and not this type, as this will be counted
/// when considering the DataModelType instead.
//
// TODO: We could attempt to do LOCAL de-duplication, for example
// a `[u8; 32]` would end up as a tuple of 32 items, drastically
// inflating the total.
pub const fn unique_types_dmt_upper(dmt: &DataModelType) -> usize {
    let child = match dmt {
        // These are all primitives
        DataModelType::Bool => 0,
        DataModelType::I8 => 0,
        DataModelType::U8 => 0,
        DataModelType::I16 => 0,
        DataModelType::I32 => 0,
        DataModelType::I64 => 0,
        DataModelType::I128 => 0,
        DataModelType::U16 => 0,
        DataModelType::U32 => 0,
        DataModelType::U64 => 0,
        DataModelType::U128 => 0,
        DataModelType::Usize => 0,
        DataModelType::Isize => 0,
        DataModelType::F32 => 0,
        DataModelType::F64 => 0,
        DataModelType::Char => 0,
        DataModelType::String => 0,
        DataModelType::ByteArray => 0,
        DataModelType::Unit => 0,
        DataModelType::Struct {
            data: Data::Unit, ..
        } => 0,
        DataModelType::Schema => 0,

        // Items with one subtype
        DataModelType::Option(dmt)
        | DataModelType::Struct {
            data: Data::Newtype(dmt),
            ..
        }
        | DataModelType::Seq(dmt) => unique_types_dmt_upper(dmt),
        // tuple-ish
        DataModelType::Tuple(dmts)
        | DataModelType::Struct {
            data: Data::Tuple(dmts),
            ..
        } => {
            let mut uniq = 0;
            let mut i = 0;
            while i < dmts.len() {
                uniq += unique_types_dmt_upper(dmts[i]);
                i += 1;
            }
            uniq
        }
        DataModelType::Map { key, val } => {
            unique_types_dmt_upper(key) + unique_types_dmt_upper(val)
        }
        DataModelType::Struct {
            data: Data::Struct(nvals),
            ..
        } => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nvals.len() {
                uniq += unique_types_dmt_upper(nvals[i].ty);
                i += 1;
            }
            uniq
        }
        DataModelType::Enum {
            variants: nvars, ..
        } => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nvars.len() {
                uniq += unique_types_var_upper(nvars[i]);
                i += 1;
            }
            uniq
        }
    };
    if is_prim(dmt) {
        child
    } else {
        child + 1
    }
}

/// Count the number of unique types contained by this Variant,
/// ONLY counting children, and not this type, as this will be counted
/// when considering the DataModelType instead.
//
// TODO: We could attempt to do LOCAL de-duplication, for example
// a `[u8; 32]` would end up as a tuple of 32 items, drastically
// inflating the total.
pub const fn unique_types_var_upper(nvar: &Variant) -> usize {
    match nvar.data {
        Data::Unit => 0,
        Data::Newtype(dmt) => unique_types_dmt_upper(dmt),
        Data::Tuple(dmts) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < dmts.len() {
                uniq += unique_types_dmt_upper(dmts[i]);
                i += 1;
            }
            uniq
        }
        Data::Struct(nvals) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nvals.len() {
                uniq += unique_types_dmt_upper(nvals[i].ty);
                i += 1;
            }
            uniq
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// STAGE 2/3 - COLLECTION OF UNIQUES AND CALCULATION OF EXACT SIZE
//////////////////////////////////////////////////////////////////////////////

/// This function collects the set of unique types, reporting the entire list
/// (which might only be partially used), as well as the *used* length.
///
/// The parameter MAX should be the highest possible number of unique types,
/// if NONE of the types have any duplication. This should be calculated using
/// [`unique_types_dmt_upper()`]. This upper bound allows us to pre-allocate
/// enough storage for the collection process.
pub const fn type_chewer_nf<const MAX: usize>(
    nf: &NamedField,
) -> ([Option<&DataModelType>; MAX], usize) {
    // Calculate the number of unique items in the children of this type
    let (mut arr, mut used) = type_chewer_dmt::<MAX>(nf.ty);
    let mut i = 0;

    // determine if this is a single-item primitive - if so, skip adding
    // this type to the unique list
    let mut found = is_prim(nf.ty);

    while !found && i < used {
        let Some(ty) = arr[i] else { panic!() };
        if dmt_eq(nf.ty, ty) {
            found = true;
        }
        i += 1;
    }
    if !found {
        arr[used] = Some(nf.ty);
        used += 1;
    }
    (arr, used)
}

/// This function collects the set of unique types, reporting the entire list
/// (which might only be partially used), as well as the *used* length.
///
/// The parameter MAX should be the highest possible number of unique types,
/// if NONE of the types have any duplication. This should be calculated using
/// [`unique_types_dmt_upper()`]. This upper bound allows us to pre-allocate
/// enough storage for the collection process.
//
// TODO: There is a LOT of duplicated code here. This is to reduce the number of
// intermediate `[Option<T>; MAX]` arrays we contain, as well as the total amount
// of recursion depth. I am open to suggestions of how to reduce this. Part of
// this restriction is that we can't take an `&mut` as a const fn arg, so we
// always have to do it by value, then merge-in the changes.
pub const fn type_chewer_dmt<const MAX: usize>(
    dmt: &DataModelType,
) -> ([Option<&DataModelType>; MAX], usize) {
    // Calculate the number of unique items in the children of this type
    let (mut arr, mut used) = match dmt {
        // These are all primitives - they never have any children to report.
        DataModelType::Bool => ([None; MAX], 0),
        DataModelType::I8 => ([None; MAX], 0),
        DataModelType::U8 => ([None; MAX], 0),
        DataModelType::I16 => ([None; MAX], 0),
        DataModelType::I32 => ([None; MAX], 0),
        DataModelType::I64 => ([None; MAX], 0),
        DataModelType::I128 => ([None; MAX], 0),
        DataModelType::U16 => ([None; MAX], 0),
        DataModelType::U32 => ([None; MAX], 0),
        DataModelType::U64 => ([None; MAX], 0),
        DataModelType::U128 => ([None; MAX], 0),
        DataModelType::Usize => ([None; MAX], 0),
        DataModelType::Isize => ([None; MAX], 0),
        DataModelType::F32 => ([None; MAX], 0),
        DataModelType::F64 => ([None; MAX], 0),
        DataModelType::Char => ([None; MAX], 0),
        DataModelType::String => ([None; MAX], 0),
        DataModelType::ByteArray => ([None; MAX], 0),
        DataModelType::Unit => ([None; MAX], 0),
        DataModelType::Schema => ([None; MAX], 0),

        // A unit struct *as a DataModelType* can be a unique/non-primitive type,
        // but DataModelType calculation is only concerned with CHILDREN, and
        // a unit struct has none.
        DataModelType::Struct {
            data: Data::Unit, ..
        } => ([None; MAX], 0),

        // Items with one subtype
        DataModelType::Option(dmt)
        | DataModelType::Struct {
            data: Data::Newtype(dmt),
            ..
        }
        | DataModelType::Seq(dmt) => type_chewer_dmt::<MAX>(dmt),
        // tuple-ish
        DataModelType::Tuple(dmts)
        | DataModelType::Struct {
            data: Data::Tuple(dmts),
            ..
        } => {
            let mut out = [None; MAX];
            let mut i = 0;
            let mut outidx = 0;

            // For each type in the tuple...
            while i < dmts.len() {
                // Get the types used by this field
                let (arr, used) = type_chewer_dmt::<MAX>(dmts[i]);
                let mut j = 0;
                // For each type in this field...
                while j < used {
                    let Some(ty) = arr[j] else { panic!() };
                    let mut k = 0;
                    let mut found = is_prim(ty);
                    // Check against all currently known tys
                    while !found && k < outidx {
                        let Some(kty) = out[k] else { panic!() };
                        found |= dmt_eq(kty, ty);
                        k += 1;
                    }
                    if !found {
                        out[outidx] = Some(ty);
                        outidx += 1;
                    }
                    j += 1;
                }
                i += 1;
            }
            (out, outidx)
        }
        DataModelType::Map { key, val } => {
            let mut out = [None; MAX];
            let mut outidx = 0;

            // Do key
            let (arr, used) = type_chewer_dmt::<MAX>(key);
            let mut j = 0;
            while j < used {
                let Some(ty) = arr[j] else { panic!() };
                let mut k = 0;
                let mut found = is_prim(ty);
                // Check against all currently known tys
                while !found && k < outidx {
                    let Some(kty) = out[k] else { panic!() };
                    found |= dmt_eq(kty, ty);
                    k += 1;
                }
                if !found {
                    out[outidx] = Some(ty);
                    outidx += 1;
                }
                j += 1;
            }

            // Then do val
            let (arr, used) = type_chewer_dmt::<MAX>(val);
            let mut j = 0;
            while j < used {
                let Some(ty) = arr[j] else { panic!() };
                let mut k = 0;
                let mut found = is_prim(ty);
                // Check against all currently known tys
                while !found && k < outidx {
                    let Some(kty) = out[k] else { panic!() };
                    found |= dmt_eq(kty, ty);
                    k += 1;
                }
                if !found {
                    out[outidx] = Some(ty);
                    outidx += 1;
                }
                j += 1;
            }

            (out, outidx)
        }
        DataModelType::Struct {
            data: Data::Struct(nfs),
            ..
        } => {
            let mut out = [None; MAX];
            let mut i = 0;
            let mut outidx = 0;

            // For each type in the tuple...
            while i < nfs.len() {
                // Get the types used by this field
                let (arr, used) = type_chewer_dmt::<MAX>(nfs[i].ty);
                let mut j = 0;
                // For each type in this field...
                while j < used {
                    let Some(ty) = arr[j] else { panic!() };
                    let mut k = 0;
                    let mut found = is_prim(ty);
                    // Check against all currently known tys
                    while !found && k < outidx {
                        let Some(kty) = out[k] else { panic!() };
                        found |= dmt_eq(kty, ty);
                        k += 1;
                    }
                    if !found {
                        out[outidx] = Some(ty);
                        outidx += 1;
                    }
                    j += 1;
                }
                i += 1;
            }
            (out, outidx)
        }
        DataModelType::Enum {
            variants: nvars, ..
        } => {
            let mut out = [None; MAX];
            let mut i = 0;
            let mut outidx = 0;

            // For each type in the variant...
            while i < nvars.len() {
                match nvars[i].data {
                    Data::Unit => {}
                    Data::Newtype(dmt) => {
                        let mut k = 0;
                        let mut found = is_prim(dmt);
                        // Check against all currently known tys
                        while !found && k < outidx {
                            let Some(kty) = out[k] else { panic!() };
                            found |= dmt_eq(kty, dmt);
                            k += 1;
                        }
                        if !found {
                            out[outidx] = Some(dmt);
                            outidx += 1;
                        }
                    }
                    Data::Tuple(dmts) => {
                        let mut x = 0;

                        // For each type in the tuple...
                        while x < dmts.len() {
                            // Get the types used by this field
                            let (arr, used) = type_chewer_dmt::<MAX>(dmts[x]);
                            let mut j = 0;
                            // For each type in this field...
                            while j < used {
                                let Some(ty) = arr[j] else { panic!() };
                                let mut k = 0;
                                let mut found = is_prim(ty);
                                // Check against all currently known tys
                                while !found && k < outidx {
                                    let Some(kty) = out[k] else { panic!() };
                                    found |= dmt_eq(kty, ty);
                                    k += 1;
                                }
                                if !found {
                                    out[outidx] = Some(ty);
                                    outidx += 1;
                                }
                                j += 1;
                            }
                            x += 1;
                        }
                    }
                    Data::Struct(nfs) => {
                        let mut x = 0;

                        // For each type in the struct...
                        while x < nfs.len() {
                            // Get the types used by this field
                            let (arr, used) = type_chewer_nf::<MAX>(nfs[x]);
                            let mut j = 0;
                            // For each type in this field...
                            while j < used {
                                let Some(ty) = arr[j] else { panic!() };
                                let mut k = 0;
                                let mut found = is_prim(ty);
                                // Check against all currently known tys
                                while !found && k < outidx {
                                    let Some(kty) = out[k] else { panic!() };
                                    found |= dmt_eq(kty, ty);
                                    k += 1;
                                }
                                if !found {
                                    out[outidx] = Some(ty);
                                    outidx += 1;
                                }
                                j += 1;
                            }
                            x += 1;
                        }
                    }
                }
                i += 1;
            }
            (out, outidx)
        }
    };
    let mut i = 0;

    // determine if this is a single-item primitive - if so, skip adding
    // this type to the unique list
    let mut found = is_prim(dmt);

    while !found && i < used {
        let Some(ty) = arr[i] else { panic!() };
        if dmt_eq(dmt, ty) {
            found = true;
        }
        i += 1;
    }
    if !found {
        arr[used] = Some(dmt);
        used += 1;
    }
    (arr, used)
}

//////////////////////////////////////////////////////////////////////////////
// STAGE 4 - REDUCTION TO CORRECT SIZE
//////////////////////////////////////////////////////////////////////////////

/// This function reduces a `&[Option<&DataModelType>]` to a `[&DataModelType; A]`.
///
/// The parameter `A` should be calculated by [`type_chewer_dmt()`].
///
/// We also validate that all items >= idx `A` are in fact None.
pub const fn cruncher<const A: usize>(
    opts: &[Option<&'static DataModelType>],
) -> [&'static DataModelType; A] {
    let mut out = [<() as Schema>::SCHEMA; A];
    let mut i = 0;
    while i < A {
        let Some(ty) = opts[i] else { panic!() };
        out[i] = ty;
        i += 1;
    }
    while i < opts.len() {
        assert!(opts[i].is_none());
        i += 1;
    }
    out
}

//////////////////////////////////////////////////////////////////////////////
// STAGE 1-5 (macro op)
//////////////////////////////////////////////////////////////////////////////

/// `unique_types` collects all unique, non-primitive types contained by the given
/// single type. It can be used with any type that implements the [`Schema`] trait,
/// and returns a `&'static [&'static DataModelType]`.
#[macro_export]
macro_rules! unique_types {
    ($t:ty) => {
        const {
            const MAX_TYS: usize =
                $crate::uniques::unique_types_dmt_upper(<$t as postcard_schema_ng::Schema>::SCHEMA);
            const BIG_RPT: (
                [Option<&'static postcard_schema_ng::schema::DataModelType>; MAX_TYS],
                usize,
            ) = $crate::uniques::type_chewer_dmt(<$t as postcard_schema_ng::Schema>::SCHEMA);
            const SMALL_RPT: [&'static postcard_schema_ng::schema::DataModelType; BIG_RPT.1] =
                $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    };
}

//////////////////////////////////////////////////////////////////////////////
// STAGE 6 - COLLECTION OF UNIQUES ACROSS MULTIPLE TYPES
//////////////////////////////////////////////////////////////////////////////

/// This function turns an array of type lists into a single list of unique types
///
/// The type parameter `M` is the maximum potential output size, it should be
/// equal to `lists.iter().map(|l| l.len()).sum()`, and should generally be
/// calculated as part of [`merge_unique_types!()`][crate::merge_unique_types].
pub const fn merge_dmt_lists<const M: usize>(
    lists: &[&[&'static DataModelType]],
) -> ([Option<&'static DataModelType>; M], usize) {
    let mut out: [Option<&DataModelType>; M] = [None; M];
    let mut out_ct = 0;
    let mut i = 0;

    while i < lists.len() {
        let mut j = 0;
        let list = lists[i];
        while j < list.len() {
            let item = list[j];
            let mut k = 0;
            let mut found = is_prim(item);
            while !found && k < out_ct {
                let Some(oitem) = out[k] else { panic!() };
                if dmt_eq(item, oitem) {
                    found = true;
                }
                k += 1;
            }
            if !found {
                out[out_ct] = Some(item);
                out_ct += 1;
            }
            j += 1;
        }
        i += 1;
    }

    (out, out_ct)
}

//////////////////////////////////////////////////////////////////////////////
// STAGE 6-9 (macro op)
//////////////////////////////////////////////////////////////////////////////

/// Get the sum of the length of all arrays
pub const fn total_len<T>(arrs: &[&[T]]) -> usize {
    let mut i = 0;
    let mut ct = 0;
    while i < arrs.len() {
        ct += arrs[i].len();
        i += 1;
    }
    ct
}

/// ,
pub const fn combine_with_copy<T: Sized + Copy, const N: usize>(arrs: &[&[T]], init: T) -> [T; N] {
    let mut out = [init; N];
    let mut outidx = 0;
    let mut i = 0;
    while i < arrs.len() {
        let mut j = 0;
        while j < arrs[i].len() {
            out[outidx] = arrs[i][j];
            outidx += 1;
            j += 1;
        }
        i += 1;
    }

    assert!(outidx == N);

    out
}

/// `merge_unique_types` collects all unique, non-primitive types contained by
/// the given comma separated types. It can be used with any types that implement
/// the [`Schema`] trait, and returns a `&'static [&'static DataModelType]`.
#[macro_export]
macro_rules! merge_unique_types {
    ($($t:ty,)*) => {
        const {
            const LISTS: &[&[&'static postcard_schema_ng::schema::DataModelType]] = &[
                $(
                    $crate::unique_types!($t),
                )*
            ];
            const TTL_COUNT: usize = $crate::uniques::total_len(LISTS);
            const BIG_RPT: ([Option<&'static postcard_schema_ng::schema::DataModelType>; TTL_COUNT], usize) = $crate::uniques::merge_dmt_lists(LISTS);
            const SMALL_RPT: [&'static postcard_schema_ng::schema::DataModelType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    }
}

#[cfg(test)]
mod test {
    #![allow(dead_code)]
    use postcard_schema_ng::{
        schema::{owned::OwnedDataModelType, DataModelType},
        Schema,
    };

    use crate::uniques::{is_prim, type_chewer_dmt, unique_types_dmt_upper};

    #[derive(Schema)]
    struct Example0;

    #[derive(Schema)]
    struct ExampleA {
        a: u32,
    }

    #[derive(Schema)]
    struct Example1 {
        a: u32,
        b: Option<u16>,
    }

    #[derive(Schema)]
    struct Example2 {
        x: i32,
        y: Option<i16>,
        c: Example1,
    }

    #[derive(Schema)]
    struct Example3 {
        a: u32,
        b: Option<u16>,
        c: Example2,
        d: Example2,
        e: Example2,
    }

    #[derive(Schema)]
    enum Example4 {
        A,
        B(String),
        C(u32, u64),
        D { x: i8, y: i16, z: i32, a: i64 },
    }

    #[test]
    fn subpar_arrs() {
        const MAXARR: usize = unique_types_dmt_upper(<[Example0; 32]>::SCHEMA);
        // I don't *like* this, it really should be 2. Leaving it as a test so
        // I can remember that it's here. See TODO on unique_types_dmt_upper.
        assert_eq!(MAXARR, 33);
    }

    #[test]
    fn uniqlo() {
        const MAX0: usize = unique_types_dmt_upper(Example0::SCHEMA);
        const MAXA: usize = unique_types_dmt_upper(ExampleA::SCHEMA);
        const MAX1: usize = unique_types_dmt_upper(Example1::SCHEMA);
        const MAX2: usize = unique_types_dmt_upper(Example2::SCHEMA);
        const MAX3: usize = unique_types_dmt_upper(Example3::SCHEMA);
        const MAX4: usize = unique_types_dmt_upper(Example4::SCHEMA);
        assert_eq!(MAX0, 1);
        assert_eq!(MAXA, 1);
        assert_eq!(MAX1, 2);
        assert_eq!(MAX2, 4);
        assert_eq!(MAX3, 14);
        assert_eq!(MAX4, 1);

        println!();
        println!("Example0");
        let (arr0, used): ([Option<_>; MAX0], usize) = type_chewer_dmt(Example0::SCHEMA);
        assert_eq!(used, 1);
        println!("max: {MAX0} used: {used}");
        for a in arr0 {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("ExampleA");
        let (arra, used): ([Option<_>; MAXA], usize) = type_chewer_dmt(ExampleA::SCHEMA);
        assert_eq!(used, 1);
        println!("max: {MAXA} used: {used}");
        for a in arra {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Option<u16>");
        let (arr1, used): (
            [Option<_>; unique_types_dmt_upper(Option::<u16>::SCHEMA)],
            usize,
        ) = type_chewer_dmt(Option::<u16>::SCHEMA);
        assert_eq!(used, 1);
        println!(
            "max: {} used: {used}",
            unique_types_dmt_upper(Option::<u16>::SCHEMA)
        );
        for a in arr1 {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Example1");
        let (arr1, used): ([Option<_>; MAX1], usize) = type_chewer_dmt(Example1::SCHEMA);
        assert!(!is_prim(Example1::SCHEMA));
        let child_ct = unique_types_dmt_upper(Example1::SCHEMA);
        assert_eq!(child_ct, 2);
        assert_eq!(used, 2);
        println!("max: {MAX1} used: {used}");
        for a in arr1 {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Example2");
        let (arr2, used): ([Option<_>; MAX2], usize) = type_chewer_dmt(Example2::SCHEMA);
        println!("max: {MAX2} used: {used}");
        for a in arr2 {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Example3");
        let (arr3, used): ([Option<_>; MAX3], usize) = type_chewer_dmt(Example3::SCHEMA);
        println!("max: {MAX3} used: {used}");
        for a in arr3 {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        let x = type_chewer_dmt::<MAX4>(Example4::SCHEMA);
        println!("Example4 {MAX4} {} {:?}", x.1, x.0);
        println!("{}", OwnedDataModelType::from(Example4::SCHEMA));
        let (arr4, used): ([Option<_>; MAX4], usize) = type_chewer_dmt(Example4::SCHEMA);
        println!("max: {MAX3} used: {used}");
        for a in arr4 {
            match a {
                Some(a) => println!("Some({})", OwnedDataModelType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        let rpt0 = unique_types!(Example0);
        println!("{}", rpt0.len());
        for a in rpt0 {
            println!("{}", OwnedDataModelType::from(*a))
        }

        println!();
        let rpta = unique_types!(ExampleA);
        println!("{}", rpta.len());
        for a in rpta {
            println!("{}", OwnedDataModelType::from(*a))
        }

        println!();
        let rpt1 = unique_types!(Example1);
        println!("{}", rpt1.len());
        for a in rpt1 {
            println!("{}", OwnedDataModelType::from(*a))
        }

        println!();
        let rpt2 = unique_types!(Example2);
        println!("{}", rpt2.len());
        for a in rpt2 {
            println!("{}", OwnedDataModelType::from(*a))
        }

        println!();
        let rpt3 = unique_types!(Example3);
        println!("{}", rpt3.len());
        for a in rpt3 {
            println!("{}", OwnedDataModelType::from(*a))
        }

        println!();
        const MERGED: &[&DataModelType] = merge_unique_types![Example3, ExampleA, Example0,];
        println!("{}", MERGED.len());
        for a in MERGED {
            println!("{}", OwnedDataModelType::from(*a))
        }

        println!();
        println!();
        println!();
        println!();

        // panic!("test passed but I want to see the data");
    }
}
