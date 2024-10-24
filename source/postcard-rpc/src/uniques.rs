// * recursively follow types, count uniques
// * make an array of options the size of uniques
// * fill in the uniques
// * make a const array of uniques
// * have a version that makes a unique array of
//   arrays

use postcard_schema::{schema::{DataModelType, DataModelVariant, NamedType, NamedValue, NamedVariant}, Schema};

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

        DataModelType::UnitStruct => false,
        // Items with one subtype
        DataModelType::Option(_) | DataModelType::NewtypeStruct(_) | DataModelType::Seq(_) => false,
        // tuple-ish
        DataModelType::Tuple(_) | DataModelType::TupleStruct(_) => false,
        DataModelType::Map { .. } => false,
        DataModelType::Struct(_) => false,
        DataModelType::Enum(_) => false,
    }
}

pub const fn merge_nty_lists<const M: usize>(lists: &[&[&'static NamedType]]) -> ([Option<&'static NamedType>; M], usize) {
    let mut out: [Option<&NamedType>; M] = [None; M];
    let mut out_ct = 0;
    let mut i = 0;

    while i < lists.len() {
        let mut j = 0;
        let list = lists[i];
        while j < list.len() {
            let item = list[j];
            let mut k = 0;
            let mut found = false;
            while !found && k < out_ct {
                let Some(oitem) = out[k] else {
                    panic!()
                };
                if nty_eq(item, oitem) {
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

// count the upper bound number of unique non-primitive types
pub const fn unique_types_nty_upper(nty: &NamedType) -> usize {
    let child_ct = unique_types_dmt_upper(nty.ty);
    if is_prim(nty.ty) {
        child_ct
    } else {
        child_ct + 1
    }
}

const fn unique_types_dmt_upper(dmt: &DataModelType) -> usize {
    match dmt {
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
        DataModelType::UnitStruct => 0,
        DataModelType::Schema => 0,

        // Items with one subtype
        DataModelType::Option(nt) | DataModelType::NewtypeStruct(nt) | DataModelType::Seq(nt) => {
            unique_types_nty_upper(nt)
        },
        // tuple-ish
        DataModelType::Tuple(nts) | DataModelType::TupleStruct(nts) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nts.len() {
                uniq += unique_types_nty_upper(nts[i]);
                i += 1;
            }
            uniq
        },
        DataModelType::Map { key, val } => {
            unique_types_nty_upper(key) + unique_types_nty_upper(val)
        },
        DataModelType::Struct(nvals) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nvals.len() {
                uniq += unique_types_nty_upper(nvals[i].ty);
                i += 1;
            }
            uniq
        },
        DataModelType::Enum(nvars) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nvars.len() {
                uniq += unique_types_var_upper(nvars[i]);
                i += 1;
            }
            uniq
        },
    }
}

const fn unique_types_var_upper(nvar: &NamedVariant) -> usize {
    match nvar.ty {
        DataModelVariant::UnitVariant => 0,
        DataModelVariant::NewtypeVariant(nt) => unique_types_nty_upper(nt),
        DataModelVariant::TupleVariant(nts) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nts.len() {
                uniq += unique_types_nty_upper(nts[i]);
                i += 1;
            }
            uniq
        },
        DataModelVariant::StructVariant(nvals) => {
            let mut uniq = 0;
            let mut i = 0;
            while i < nvals.len() {
                uniq += unique_types_nty_upper(nvals[i].ty);
                i += 1;
            }
            uniq
        },
    }
}

pub const fn type_chewer_nty<const MAX: usize>(nty: &NamedType) -> ([Option<&NamedType>; MAX], usize) {
    let (mut arr, mut used) = type_chewer_dmt::<MAX>(nty.ty);
    let mut i = 0;

    // determine if this is a single-item primitive
    let mut found = is_prim(nty.ty);

    while !found && i < used {
        let Some(ty) = arr[i] else {
            panic!()
        };
        if nty_eq(nty, ty) {
            found = true;
        }
        i += 1;
    }
    if !found {
        arr[used] = Some(nty);
        used += 1;
    }
    (arr, used)
}

const fn type_chewer_dmt<const MAX: usize>(dmt: &DataModelType) -> ([Option<&NamedType>; MAX], usize) {
    match dmt {
        // These are all primitives
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
        DataModelType::UnitStruct => ([None; MAX], 0),
        DataModelType::Schema => ([None; MAX], 0),

        // Items with one subtype
        DataModelType::Option(nt) | DataModelType::NewtypeStruct(nt) | DataModelType::Seq(nt) => {
            type_chewer_nty::<MAX>(nt)
        },
        // tuple-ish
        DataModelType::Tuple(nts) | DataModelType::TupleStruct(nts) => {
            let mut out = [None; MAX];
            let mut i = 0;
            let mut outidx = 0;

            // For each type in the tuple...
            while i < nts.len() {
                // Get the types used by this field
                let (arr, used) = type_chewer_nty::<MAX>(nts[i]);
                let mut j = 0;
                // For each type in this field...
                while j < used {
                    let Some(ty) = arr[j] else {
                        panic!()
                    };
                    let mut k = 0;
                    let mut found = false;
                    // Check against all currently known tys
                    while !found && k < outidx {
                        let Some(kty) = out[k] else {
                            panic!()
                        };
                        found |= nty_eq(kty, ty);
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
        },
        DataModelType::Map { key, val } => {
            let mut out = [None; MAX];
            let mut outidx = 0;

            // Do key
            let (arr, used) = type_chewer_nty::<MAX>(key);
            let mut j = 0;
            while j < used {
                let Some(ty) = arr[j] else {
                    panic!()
                };
                let mut k = 0;
                let mut found = false;
                // Check against all currently known tys
                while !found && k < outidx {
                    let Some(kty) = out[k] else {
                        panic!()
                    };
                    found |= nty_eq(kty, ty);
                    k += 1;
                }
                if !found {
                    out[outidx] = Some(ty);
                    outidx += 1;
                }
                j += 1;
            }

            // Then do val
            let (arr, used) = type_chewer_nty::<MAX>(val);
            let mut j = 0;
            while j < used {
                let Some(ty) = arr[j] else {
                    panic!()
                };
                let mut k = 0;
                let mut found = false;
                // Check against all currently known tys
                while !found && k < outidx {
                    let Some(kty) = out[k] else {
                        panic!()
                    };
                    found |= nty_eq(kty, ty);
                    k += 1;
                }
                if !found {
                    out[outidx] = Some(ty);
                    outidx += 1;
                }
                j += 1;
            }

            (out, outidx)
        },
        DataModelType::Struct(nvals) => {
            let mut out = [None; MAX];
            let mut i = 0;
            let mut outidx = 0;

            // For each type in the tuple...
            while i < nvals.len() {
                // Get the types used by this field
                let (arr, used) = type_chewer_nty::<MAX>(nvals[i].ty);
                let mut j = 0;
                // For each type in this field...
                while j < used {
                    let Some(ty) = arr[j] else {
                        panic!()
                    };
                    let mut k = 0;
                    let mut found = false;
                    // Check against all currently known tys
                    while !found && k < outidx {
                        let Some(kty) = out[k] else {
                            panic!()
                        };
                        found |= nty_eq(kty, ty);
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
        },
        DataModelType::Enum(nvars) => {
            let mut out = [None; MAX];
            let mut i = 0;
            let mut outidx = 0;

            // For each type in the tuple...
            while i < nvars.len() {
                match nvars[i].ty {
                    DataModelVariant::UnitVariant => continue,
                    DataModelVariant::NewtypeVariant(nt) => {
                        let mut k = 0;
                        let mut found = false;
                        // Check against all currently known tys
                        while !found && k < outidx {
                            let Some(kty) = out[k] else {
                                panic!()
                            };
                            found |= nty_eq(kty, nt);
                            k += 1;
                        }
                        if !found {
                            out[outidx] = Some(nt);
                            outidx += 1;
                        }
                    },
                    DataModelVariant::TupleVariant(nts) => {
                        let mut x = 0;

                        // For each type in the tuple...
                        while x < nts.len() {
                            // Get the types used by this field
                            let (arr, used) = type_chewer_nty::<MAX>(nts[x]);
                            let mut j = 0;
                            // For each type in this field...
                            while j < used {
                                let Some(ty) = arr[j] else {
                                    panic!()
                                };
                                let mut k = 0;
                                let mut found = false;
                                // Check against all currently known tys
                                while !found && k < outidx {
                                    let Some(kty) = out[k] else {
                                        panic!()
                                    };
                                    found |= nty_eq(kty, ty);
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
                    },
                    DataModelVariant::StructVariant(nvals) => {
                        let mut x = 0;

                        // For each type in the tuple...
                        while x < nvals.len() {
                            // Get the types used by this field
                            let (arr, used) = type_chewer_nty::<MAX>(nvals[x].ty);
                            let mut j = 0;
                            // For each type in this field...
                            while j < used {
                                let Some(ty) = arr[j] else {
                                    panic!()
                                };
                                let mut k = 0;
                                let mut found = false;
                                // Check against all currently known tys
                                while !found && k < outidx {
                                    let Some(kty) = out[k] else {
                                        panic!()
                                    };
                                    found |= nty_eq(kty, ty);
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
                    },
                }
                i += 1;
            }
            (out, outidx)
        },
    }
}

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

const fn nty_eq(a: &NamedType, b: &NamedType) -> bool {
    str_eq(a.name, b.name) && dmt_eq(a.ty, b.ty)
}

const fn ntys_eq(a: &[&NamedType], b: &[&NamedType]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if !nty_eq(a[i], b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

const fn dmt_eq(a: &DataModelType, b: &DataModelType) -> bool {
    match (a, b) {
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
        (DataModelType::UnitStruct, DataModelType::UnitStruct) => true,
        (DataModelType::Schema, DataModelType::Schema) => true,

        (DataModelType::Option(nta), DataModelType::Option(ntb)) => nty_eq(nta, ntb),
        (DataModelType::NewtypeStruct(nta), DataModelType::NewtypeStruct(ntb)) => nty_eq(nta, ntb),
        (DataModelType::Seq(nta), DataModelType::Seq(ntb)) => nty_eq(nta, ntb),

        (DataModelType::Tuple(ntsa), DataModelType::Tuple(ntsb)) => ntys_eq(ntsa, ntsb),
        (DataModelType::TupleStruct(ntsa), DataModelType::TupleStruct(ntsb)) => ntys_eq(ntsa, ntsb),
        (DataModelType::Map { key: keya, val: vala }, DataModelType::Map { key: keyb, val: valb }) => {
            nty_eq(keya, keyb) && nty_eq(vala, valb)
        }
        (DataModelType::Struct(nvalsa), DataModelType::Struct(nvalsb)) => vals_eq(nvalsa, nvalsb),
        (DataModelType::Enum(nvarsa), DataModelType::Enum(nvarsb)) => vars_eq(nvarsa, nvarsb),
        _ => false
    }
}



const fn var_eq(a: &NamedVariant, b: &NamedVariant) -> bool {
    if !str_eq(a.name, b.name) {
        return false;
    }
    dmv_eq(a.ty, b.ty)
}

const fn vars_eq(a: &[&NamedVariant], b: &[&NamedVariant]) -> bool {
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

const fn vals_eq(a: &[&NamedValue], b: &[&NamedValue]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if !str_eq(a[i].name, b[i].name) {
            return false;
        }
        if !nty_eq(a[i].ty, b[i].ty) {
            return false;
        }

        i += 1;
    }
    true
}

const fn dmv_eq(a: &DataModelVariant, b: &DataModelVariant) -> bool {
    match (a, b) {
        (DataModelVariant::UnitVariant, DataModelVariant::UnitVariant) => true,
        (DataModelVariant::NewtypeVariant(nta), DataModelVariant::NewtypeVariant(ntb)) => nty_eq(nta, ntb),
        (DataModelVariant::TupleVariant(ntsa), DataModelVariant::TupleVariant(ntsb)) => ntys_eq(ntsa, ntsb),
        (DataModelVariant::StructVariant(nvarsa), DataModelVariant::StructVariant(nvarsb)) => vals_eq(nvarsa, nvarsb),
        _ => false,
    }
}

pub const fn cruncher<const A: usize>(opts: &[Option<&'static NamedType>]) -> [&'static NamedType; A] {
    let mut out = [<() as Schema>::SCHEMA; A];
    let mut i = 0;
    while i < A {
        let Some(ty) = opts[i] else {
            panic!()
        };
        out[i] = ty;
        i += 1;
    }
    while i < opts.len() {
        assert!(opts[i].is_none());
        i += 1;
    }
    out
}

#[macro_export]
macro_rules! unique_types {
    ($t:ty) => {
        const {
            const MAX_TYS: usize = $crate::uniques::unique_types_nty_upper(<$t as postcard_schema::Schema>::SCHEMA);
            const BIG_RPT: ([Option<&'static postcard_schema::schema::NamedType>; MAX_TYS], usize) = $crate::uniques::type_chewer_nty(<$t as postcard_schema::Schema>::SCHEMA);
            const SMALL_RPT: [&'static postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    };
}

#[macro_export]
macro_rules! merge_unique_types {
    ($($t:ty,)*) => {
        const {
            const LISTS: &[&[&'static postcard_schema::schema::NamedType]] = &[
                $(
                    $crate::unique_types!($t),
                )*
            ];
            const TTL_COUNT: usize = const {
                let mut i = 0;
                let mut ct = 0;
                while i < LISTS.len() {
                    ct += LISTS[i].len();
                    i += 1;
                }
                ct
            };
            const BIG_RPT: ([Option<&'static postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(LISTS);
            const SMALL_RPT: [&'static postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    }
}

#[cfg(test)]
mod test {
    use postcard_schema::{schema::{owned::OwnedNamedType, NamedType}, Schema};

    use crate::uniques::{is_prim, type_chewer_nty, unique_types_dmt_upper, unique_types_nty_upper};

    // Should have an upper of 1
    #[derive(Schema)]
    struct Example0;

    // Should have an upper of 1
    #[derive(Schema)]
    struct ExampleA {
        a: u32,
    }

    // Should have an upper of 2?
    #[derive(Schema)]
    struct Example1 {
        a: u32,
        b: Option<u16>,
    }

    // Should have an upper of 2
    #[derive(Schema)]
    struct Example2 {
        x: i32,
        y: Option<i16>,
        c: Example1,
    }

    // Should have an upper of... a lot?
    #[derive(Schema)]
    struct Example3 {
        a: u32,
        b: Option<u16>,
        c: Example2,
        d: Example2,
        e: Example2,
    }

    // #[test]
    // fn uniqhi() {
    //     // let upper = unique_types_nty_upper(Example0::SCHEMA);
    //     // println!("{}", OwnedNamedType::from(Example0::SCHEMA));
    //     // println!("upper: {upper}");

    //     // const MAX0: usize = unique_types_nty_upper(Example0::SCHEMA);
    //     // let (arr0, used): ([Option<_>; MAX0], usize) = type_chewer_nty(Example0::SCHEMA);
    //     // println!("used: {used}");
    //     // for a in arr0 {
    //     //     match a {
    //     //         Some(a) => println!("Some({})", OwnedNamedType::from(a)),
    //     //         None => println!("None"),
    //     //     }
    //     // }

    //     // println!("---");

    //     let upper = unique_types_nty_upper(ExampleA::SCHEMA);
    //     println!("{}", OwnedNamedType::from(ExampleA::SCHEMA));
    //     println!("upper: {upper}");

    //     const MAXA: usize = unique_types_nty_upper(ExampleA::SCHEMA);
    //     let (arra, used): ([Option<_>; MAXA], usize) = type_chewer_nty(ExampleA::SCHEMA);
    //     println!("used: {used}");
    //     for a in arra {
    //         match a {
    //             Some(a) => println!("Some({})", OwnedNamedType::from(a)),
    //             None => println!("None"),
    //         }
    //     }

    //     panic!()
    // }

    #[test]
    fn uniqlo() {
        const MAX0: usize = unique_types_nty_upper(Example0::SCHEMA);
        const MAXA: usize = unique_types_nty_upper(ExampleA::SCHEMA);
        const MAX1: usize = unique_types_nty_upper(Example1::SCHEMA);
        const MAX2: usize = unique_types_nty_upper(Example2::SCHEMA);
        const MAX3: usize = unique_types_nty_upper(Example3::SCHEMA);
        assert_eq!(MAX0, 1);
        assert_eq!(MAXA, 1);
        assert_eq!(MAX1, 2);

        println!();
        println!("Example0");
        let (arr0, used): ([Option<_>; MAX0], usize) = type_chewer_nty(Example0::SCHEMA);
        assert_eq!(used, 1);
        println!("max: {MAX0} used: {used}");
        for a in arr0 {
            match a {
                Some(a) => println!("Some({})", OwnedNamedType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("ExampleA");
        let (arra, used): ([Option<_>; MAXA], usize) = type_chewer_nty(ExampleA::SCHEMA);
        assert_eq!(used, 1);
        println!("max: {MAXA} used: {used}");
        for a in arra {
            match a {
                Some(a) => println!("Some({})", OwnedNamedType::from(a)),
                None => println!("None"),
            }
        }


        println!();
        println!("Option<u16>");
        let (arr1, used): ([Option<_>; unique_types_nty_upper(Option::<u16>::SCHEMA)], usize) = type_chewer_nty(Option::<u16>::SCHEMA);
        assert_eq!(used, 1);
        println!("max: {} used: {used}", unique_types_nty_upper(Option::<u16>::SCHEMA));
        for a in arr1 {
            match a {
                Some(a) => println!("Some({})", OwnedNamedType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Example1");
        let (arr1, used): ([Option<_>; MAX1], usize) = type_chewer_nty(Example1::SCHEMA);
        assert!(!is_prim(Example1::SCHEMA.ty));
        let child_ct = unique_types_dmt_upper(Example1::SCHEMA.ty);
        assert_eq!(child_ct, 1);
        assert_eq!(used, 2);
        println!("max: {MAX1} used: {used}");
        for a in arr1 {
            match a {
                Some(a) => println!("Some({})", OwnedNamedType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Example2");
        let (arr2, used): ([Option<_>; MAX2], usize) = type_chewer_nty(Example2::SCHEMA);
        println!("max: {MAX2} used: {used}");
        for a in arr2 {
            match a {
                Some(a) => println!("Some({})", OwnedNamedType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        println!("Example3");
        let (arr3, used): ([Option<_>; MAX3], usize) = type_chewer_nty(Example3::SCHEMA);
        println!("max: {MAX3} used: {used}");
        for a in arr3 {
            match a {
                Some(a) => println!("Some({})", OwnedNamedType::from(a)),
                None => println!("None"),
            }
        }

        println!();
        let rpt0 = unique_types!(Example0);
        println!("{}", rpt0.len());
        for a in rpt0 {
            println!("{}", OwnedNamedType::from(*a))
        }

        println!();
        let rpta = unique_types!(ExampleA);
        println!("{}", rpta.len());
        for a in rpta {
            println!("{}", OwnedNamedType::from(*a))
        }

        println!();
        let rpt1 = unique_types!(Example1);
        println!("{}", rpt1.len());
        for a in rpt1 {
            println!("{}", OwnedNamedType::from(*a))
        }

        println!();
        let rpt2 = unique_types!(Example2);
        println!("{}", rpt2.len());
        for a in rpt2 {
            println!("{}", OwnedNamedType::from(*a))
        }

        println!();
        let rpt3 = unique_types!(Example3);
        println!("{}", rpt3.len());
        for a in rpt3 {
            println!("{}", OwnedNamedType::from(*a))
        }

        println!();
        const MERGED: &[&NamedType] = merge_unique_types![
            Example3,
            ExampleA,
            Example0,
        ];
        println!("{}", MERGED.len());
        for a in MERGED {
            println!("{}", OwnedNamedType::from(*a))
        }

        panic!("test passed but I want to see the data");
    }
}
