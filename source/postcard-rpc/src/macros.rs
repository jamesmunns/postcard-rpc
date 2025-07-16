/// ## Endpoint macro
///
/// Used to define a single Endpoint marker type that implements the
/// [Endpoint][crate::Endpoint] trait.
///
/// Prefer the [`endpoints!()`][crate::endpoints] macro instead.
///
/// ```rust
/// # use postcard_schema::Schema;
/// # use serde::{Serialize, Deserialize};
/// use postcard_rpc::endpoint;
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Req1 {
///     a: u8,
///     b: u64,
/// }
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Resp1 {
///     c: [u8; 4],
///     d: i32,
/// }
///
/// endpoint!(Endpoint1, Req1, Resp1, "endpoint/1");
/// ```
///
/// If the path is omitted, the type name is used instead.
#[macro_export]
macro_rules! endpoint {
    ($tyname:ident, $req:ty, $resp:ty) => {
        endpoint!($tyname, $req, $resp, stringify!($tyname));
    };
    ($tyname:ident, $req:ty, $resp:ty, $path:expr,) => {
        endpoint!($tyname, $req, $resp, $path)
    };
    ($tyname:ident, $req:ty, $resp:ty, $path:expr) => {
        pub struct $tyname;

        impl $crate::Endpoint for $tyname {
            type Request = $req;
            type Response = $resp;
            const PATH: &'static str = $path;
            const REQ_KEY: $crate::Key = $crate::Key::for_path::<$req>($path);
            const RESP_KEY: $crate::Key = $crate::Key::for_path::<$resp>($path);
        }
    };
}

/// ## Endpoints macro
///
/// Used to define multiple Endpoint marker types that implements the
/// [Endpoint][crate::Endpoint] trait.
///
/// ```rust
/// # use postcard_schema::Schema;
/// # use serde::{Serialize, Deserialize};
/// use postcard_rpc::endpoints;
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Req1 {
///     a: u8,
///     b: u64,
/// }
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Resp1 {
///     c: [u8; 4],
///     d: i32,
/// }
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Req2 {
///     a: i8,
///     b: i64,
/// }
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Resp2 {
///     c: [i8; 4],
///     d: u32,
/// }
///
/// endpoints!{
///     list = ENDPOINTS_LIST;
///     | EndpointTy     | RequestTy     | ResponseTy    | Path              |
///     | ----------     | ---------     | ----------    | ----              |
///     | Endpoint1      | Req1          | Resp1         | "endpoints/one"   |
///     | Endpoint2      | Req2          | Resp2         | "endpoints/two"   |
/// }
/// ```
#[macro_export]
macro_rules! endpoints {
    (@ep_tys $([[$($meta:meta)?] $ep_name:ident])*) => {
        $crate::endpoints!(@ep_tys omit_std=false; $([[$($meta)?] $ep_name])*)
    };
    (@ep_tys omit_std=true; $([[$($meta:meta)?] $ep_name:ident])*) => {
        const {
            const LISTS: &[&[&'static $crate::postcard_schema::schema::NamedType]] = &[
                $(
                    $(#[$meta])?
                    $crate::unique_types!(<$ep_name as $crate::Endpoint>::Request),
                    $(#[$meta])?
                    $crate::unique_types!(<$ep_name as $crate::Endpoint>::Response),
                )*
            ];

            const TTL_COUNT: usize = $crate::uniques::total_len(LISTS);
            const BIG_RPT: ([Option<&'static $crate::postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(LISTS);
            const SMALL_RPT: [&'static $crate::postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    };
    (@ep_tys omit_std=false; $([[$($meta:meta)?] $ep_name:ident])*) => {
        const {
            const USER_TYS: &[&'static $crate::postcard_schema::schema::NamedType] =
                $crate::endpoints!(@ep_tys omit_std=true; $([[$($meta)?] $ep_name])*);
            const STD_TYS: &[&'static $crate::postcard_schema::schema::NamedType]
                = $crate::standard_icd::STANDARD_ICD_ENDPOINTS.types;

            const BOTH: &[&[&'static $crate::postcard_schema::schema::NamedType]] = &[
                USER_TYS, STD_TYS,
            ];
            const TTL_COUNT: usize = $crate::uniques::total_len(BOTH);
            const BIG_RPT: ([Option<&'static $crate::postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(BOTH);
            const SMALL_RPT: [&'static $crate::postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    };
    (@ep_eps $([[$($meta:meta)?] $ep_name:ident])*) => {
        $crate::endpoints!(@ep_eps omit_std=false; $([[$($meta)?] $ep_name])*)
    };
    (@ep_eps omit_std=true; $([[$($meta:meta)?] $ep_name:ident])*) => {
        &[
            $(
                $(#[$meta])?
                (
                    <$ep_name as $crate::Endpoint>::PATH,
                    <$ep_name as $crate::Endpoint>::REQ_KEY,
                    <$ep_name as $crate::Endpoint>::RESP_KEY,
                ),
            )*
        ]
    };
    (@ep_eps omit_std=false; $([[$($meta:meta)?] $ep_name:ident])*) => {
        const {
            const USER_EPS: &[(&str, $crate::Key, $crate::Key)] =
                $crate::endpoints!(@ep_eps omit_std=true; $([[$($meta)?] $ep_name])*);
            const NULL_KEY: $crate::Key = unsafe { $crate::Key::from_bytes([0u8; 8]) };
            const STD_EPS: &[(&str, $crate::Key, $crate::Key)] =
                $crate::standard_icd::STANDARD_ICD_ENDPOINTS.endpoints;

            $crate::concat_arrays! {
                init = ("", NULL_KEY, NULL_KEY);
                ty = (&str, $crate::Key, $crate::Key);
                [STD_EPS, USER_EPS]
            }
        }
    };
    (
           list = $list_name:ident;
           $(omit_std = $omit:tt;)?
           | EndpointTy     | RequestTy                                | ResponseTy                                  | Path              | $( Cfg           |)?
           | $(-)*          | $(-)*                                    | $(-)*                                       | $(-)*             | $($(-)*          |)?
        $( | $ep_name:ident | $req_ty:tt $(< $($req_lt:lifetime),+ >)? | $resp_ty:tt $(< $($resp_lt:lifetime),+ >)?  | $path_str:literal | $($meta:meta)? $(|)? )*
    ) => {
        // struct definitions and trait impls
        $(
            /// Macro Generated Marker Type
            $(#[$meta])?
            pub struct $ep_name < $($($req_lt,)+)? $($($resp_lt,)+)? > {
                $(
                    _plt_req: core::marker::PhantomData<($(& $req_lt (),)+)>,
                )?
                $(
                    _plt_resp: core::marker::PhantomData<($(& $resp_lt (),)+)>,
                )?
                _priv: core::marker::PhantomData<()>,
            }

            $(#[$meta])?
            impl < $($($req_lt,)+)? $($($resp_lt,)+)? > $crate::Endpoint for $ep_name < $($($req_lt,)+)? $($($resp_lt,)+)? > {
                type Request = $req_ty $(< $($req_lt,)+ >)?;
                type Response = $resp_ty $(< $($resp_lt,)+ >)?;
                const PATH: &'static str = $path_str;
                const REQ_KEY: $crate::Key = $crate::Key::for_path::<$req_ty>($path_str);
                const RESP_KEY: $crate::Key = $crate::Key::for_path::<$resp_ty>($path_str);
            }
        )*

        /// Macro Generated Endpoint Map
        pub const $list_name: $crate::EndpointMap = $crate::EndpointMap {
            types: $crate::endpoints!(@ep_tys $(omit_std = $omit;)? $([[$($meta)?] $ep_name])*),
            endpoints: $crate::endpoints!(@ep_eps $(omit_std = $omit;)? $([[$($meta)?] $ep_name])*),
        };
    };
}

/// ## Topic macro
///
/// Used to define a single Topic marker type that implements the
/// [Topic][crate::Topic] trait.
///
/// Prefer the [`topics!()` macro](crate::topics) macro instead.
///
/// ```rust
/// # use postcard_schema::Schema;
/// # use serde::{Serialize, Deserialize};
/// use postcard_rpc::topic;
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Message1 {
///     a: u8,
///     b: u64,
/// }
///
/// topic!(Topic1, Message1, "topic/1");
/// ```
///
/// If the path is omitted, the type name is used instead.
#[macro_export]
macro_rules! topic {
    ($tyname:ident, $msg:ty) => {
        topic!($tyname, $msg, stringify!($tyname));
    };
    ($tyname:ident, $msg:ty, $path:expr,) => {
        topic!($tyname, $msg, $path)
    };
    ($tyname:ident, $msg:ty, $path:expr) => {
        /// $tyname - A Topic definition type
        ///
        /// Generated by the `topic!()` macro
        pub struct $tyname;

        impl $crate::Topic for $tyname {
            type Message = $msg;
            const PATH: &'static str = $path;
            const TOPIC_KEY: $crate::Key = $crate::Key::for_path::<$msg>($path);
        }
    };
}

/// ## Topics macro
///
/// Used to define multiple Topic marker types that implements the
/// [Topic][crate::Topic] trait.
///
/// ```rust
/// # use postcard_schema::Schema;
/// # use serde::{Serialize, Deserialize};
/// use postcard_rpc::{topics, TopicDirection};
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Message1 {
///     a: u8,
///     b: u64,
/// }
///
/// #[derive(Debug, Serialize, Deserialize, Schema)]
/// pub struct Message2 {
///     a: i8,
///     b: i64,
/// }
///
/// topics!{
///    list = TOPIC_LIST_NAME;
///    direction = TopicDirection::ToServer;
///    | TopicTy        | MessageTy     | Path              |
///    | -------        | ---------     | ----              |
///    | Topic1         | Message1      | "topics/one"      |
///    | Topic2         | Message2      | "topics/two"      |
/// }
/// ```
#[macro_export]
macro_rules! topics {
    (@tp_tys ( $dir:expr ) $([[$($meta:meta)?] $tp_name:ident])*) => {
        $crate::topics!(@tp_tys ( $dir ) omit_std=false; $([[$($meta)?] $tp_name])*)
    };
    (@tp_tys ( $dir:expr ) omit_std=true; $([[$($meta:meta)?] $tp_name:ident])*) => {
        const {
            const LISTS: &[&[&'static $crate::postcard_schema::schema::NamedType]] = &[
                $(
                    $(#[$meta])?
                    $crate::unique_types!(<$tp_name as $crate::Topic>::Message),
                )*
            ];

            const TTL_COUNT: usize = $crate::uniques::total_len(LISTS);
            const BIG_RPT: ([Option<&'static $crate::postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(LISTS);
            const SMALL_RPT: [&'static $crate::postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    };
    (@tp_tys ( $dir:expr ) omit_std=false; $([[$($meta:meta)?] $tp_name:ident])*) => {
        const {
            const USER_TYS: &[&'static $crate::postcard_schema::schema::NamedType] =
                $crate::topics!(@tp_tys ( $dir ) omit_std=true; $([[$($meta)?] $tp_name])*);
            const STD_TYS: &[&'static $crate::postcard_schema::schema::NamedType] = const {
                match $dir {
                    $crate::TopicDirection::ToServer => $crate::standard_icd::STANDARD_ICD_TOPICS_IN.types,
                    $crate::TopicDirection::ToClient => $crate::standard_icd::STANDARD_ICD_TOPICS_OUT.types,
                }
            };

            const BOTH: &[&[&'static $crate::postcard_schema::schema::NamedType]] = &[
                STD_TYS, USER_TYS,
            ];
            const TTL_COUNT: usize = $crate::uniques::total_len(BOTH);
            const BIG_RPT: ([Option<&'static $crate::postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(BOTH);
            const SMALL_RPT: [&'static $crate::postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
            SMALL_RPT.as_slice()
        }
    };
    (@tp_tps ( $dir:expr ) $([[$($meta:meta)?] $tp_name:ident])*) => {
        $crate::topics!(@tp_tps ( $dir ) omit_std=false; $([[$($meta)?] $tp_name])*)
    };
    (@tp_tps ( $dir:expr ) omit_std=true; $([[$($meta:meta)?] $tp_name:ident])*) => {
        &[
            $(
                $(#[$meta])?
                (
                    <$tp_name as $crate::Topic>::PATH,
                    <$tp_name as $crate::Topic>::TOPIC_KEY,
                ),
            )*
        ]
    };
    (@tp_tps ( $dir:expr ) omit_std=false; $([[$($meta:meta)?] $tp_name:ident])*) => {
        const {
            const USER_TPS: &[(&str, $crate::Key)] =
                $crate::topics!(@tp_tps ( $dir ) omit_std=true; $([[$($meta)?] $tp_name])*);
            const NULL_KEY: $crate::Key = unsafe { $crate::Key::from_bytes([0u8; 8]) };
            const STD_TPS: &[(&str, $crate::Key)] = const {
                match $dir {
                    $crate::TopicDirection::ToServer => $crate::standard_icd::STANDARD_ICD_TOPICS_IN.topics,
                    $crate::TopicDirection::ToClient => $crate::standard_icd::STANDARD_ICD_TOPICS_OUT.topics,
                }
            };

            $crate::concat_arrays! {
                init = ("", NULL_KEY);
                ty = (&str, $crate::Key);
                [STD_TPS, USER_TPS]
            }
        }
    };
    (
        list = $list_name:ident;
        direction = $direction:expr;
        $(omit_std = $omit:tt;)?
        | TopicTy        | MessageTy                                | Path              | $( Cfg           |)?
        | $(-)*          | $(-)*                                    | $(-)*             | $($(-)*          |)?
      $(| $tp_name:ident | $msg_ty:tt $(< $($msg_lt:lifetime),+ >)? | $path_str:literal | $($meta:meta)? $(|)?)*
    ) => {
        // struct definitions and trait impls
        $(
            /// $tp_name - A Topic definition type
            ///
            /// Generated by the `topics!()` macro
            $(#[$meta])?
            pub struct $tp_name $(< $($msg_lt,)+ >)? {
                $(
                    _plt: core::marker::PhantomData<($(& $msg_lt (),)+)>,
                )?
                _priv: core::marker::PhantomData<()>,
            }

            $(#[$meta])?
            impl $(< $($msg_lt),+ >)? $crate::Topic for $tp_name $(< $($msg_lt,)+ >)? {
                type Message = $msg_ty $(< $($msg_lt,)+ >)?;
                const PATH: &'static str = $path_str;
                const TOPIC_KEY: $crate::Key = $crate::Key::for_path::<$msg_ty>($path_str);
            }
        )*

        /// Macro Generated Topic Map
        pub const $list_name: $crate::TopicMap = $crate::TopicMap {
            direction: $direction,
            types: $crate::topics!(@tp_tys ( $direction ) $(omit_std = $omit;)? $([[$($meta)?] $tp_name])*),
            topics: $crate::topics!(@tp_tps ( $direction ) $(omit_std = $omit;)? $([[$($meta)?] $tp_name])*),
        };
    };
}

/// A macro for turning `&[&[T]]` into `&[T]`
#[macro_export]
macro_rules! concat_arrays {
    (
        init = $init:expr;
        ty = $tyname:ty;
        [$($arr:ident),+]
    ) => {
        const {
            const SLI: &[&[$tyname]] = &[
                $($arr,)+
            ];
            const LEN: usize = $crate::uniques::total_len(SLI);
            const ARR: [$tyname; LEN] = $crate::uniques::combine_with_copy(SLI, $init);

            ARR.as_slice()
        }
    };
}

#[cfg(test)]
mod concat_test {
    #[test]
    fn concats() {
        const A: &[u32] = &[1, 2, 3];
        const B: &[u32] = &[4, 5, 6];
        const BOTH: &[u32] = concat_arrays!(
            init = 0xFFFF_FFFF;
            ty = u32;
            [A, B]
        );
        assert_eq!(BOTH, [1, 2, 3, 4, 5, 6]);
    }
}

/// A helper function for logging with the [Sender][crate::server::Sender]
#[macro_export]
macro_rules! sender_fmt {
    ($sender:ident, $($arg:tt)*) => {
        $sender.log_fmt(format_args!($($arg)*))
    };
    ($($arg:tt)*) => {
        compile_error!("You must pass the sender to `sender_log`!");
    }
}

#[cfg(test)]
mod endpoints_test {
    use postcard_schema::{schema::owned::OwnedNamedType, Schema};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Schema)]
    pub struct AReq(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct AResp(pub u16);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BTopic(pub u32);

    endpoints! {
        list = ENDPOINT_LIST;
        | EndpointTy     | RequestTy     | ResponseTy    | Path              |
        | ----------     | ---------     | ----------    | ----              |
        | AlphaEndpoint1 | AReq          | AResp         | "test/alpha1"     |
        | AlphaEndpoint2 | AReq          | AResp         | "test/alpha2"     |
        | AlphaEndpoint3 | AReq          | AResp         | "test/alpha3"     |
    }

    topics! {
        list = TOPICS_IN_LIST;
        direction = crate::TopicDirection::ToServer;
        | TopicTy        | MessageTy     | Path              |
        | ----------     | ---------     | ----              |
        | BetaTopic1     | BTopic        | "test/in/beta1"   |
        | BetaTopic2     | BTopic        | "test/in/beta2"   |
        | BetaTopic3     | BTopic        | "test/in/beta3"   |
    }

    topics! {
        list = TOPICS_OUT_LIST;
        direction = crate::TopicDirection::ToClient;
        | TopicTy        | MessageTy     | Path              |
        | ----------     | ---------     | ----              |
        | BetaTopic4     | BTopic        | "test/out/beta1"  |
    }

    #[test]
    fn eps() {
        for ep in ENDPOINT_LIST.types {
            println!("{}", OwnedNamedType::from(*ep));
        }
        assert_eq!(ENDPOINT_LIST.types.len(), 3);
        for ep in ENDPOINT_LIST.endpoints {
            println!("{}", ep.0);
        }
        assert_eq!(ENDPOINT_LIST.endpoints.len(), 5);
    }

    #[test]
    fn tps() {
        for tp in TOPICS_IN_LIST.types {
            println!("TY IN:  {}", OwnedNamedType::from(*tp));
        }
        for tp in TOPICS_IN_LIST.topics {
            println!("TP IN:  {}", tp.0);
        }
        for tp in TOPICS_OUT_LIST.types {
            println!("TY OUT: {}", OwnedNamedType::from(*tp));
        }
        for tp in TOPICS_OUT_LIST.topics {
            println!("TP OUT: {}", tp.0);
        }
        assert_eq!(TOPICS_IN_LIST.types.len(), 1);
        assert_eq!(TOPICS_IN_LIST.topics.len(), 3);
        assert_eq!(TOPICS_OUT_LIST.types.len(), 5);
        assert_eq!(TOPICS_OUT_LIST.topics.len(), 3);
    }
}
