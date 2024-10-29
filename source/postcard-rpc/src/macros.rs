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
    (
           list = $list_name:ident;
           | EndpointTy     | RequestTy                                | ResponseTy                                  | Path              | $( Cfg           |)?
           | $(-)*          | $(-)*                                    | $(-)*                                       | $(-)*             | $($(-)*          |)?
        $( | $ep_name:ident | $req_ty:tt $(< $($req_lt:lifetime),+ >)? | $resp_ty:tt $(< $($resp_lt:lifetime),+ >)?  | $path_str:literal | $($meta:meta)? $(|)? )*
    ) => {
        // struct definitions and trait impls
        $(
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

        pub const $list_name: $crate::EndpointMap = $crate::EndpointMap {
            types: &[
                $(
                    $(#[$meta])?
                    <$ep_name as $crate::Endpoint>::Request::SCHEMA,
                    $(#[$meta])?
                    <$ep_name as $crate::Endpoint>::Response::SCHEMA,
                )*
            ],
            endpoints: &[
                $(
                    $(#[$meta])?
                    (
                        <$ep_name as $crate::Endpoint>::PATH,
                        <$ep_name as $crate::Endpoint>::REQ_KEY,
                        <$ep_name as $crate::Endpoint>::RESP_KEY,
                    ),
                )*
            ],
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
/// use postcard_rpc::topics;
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
///    | TopicTy        | MessageTy     | Path              |
///    | -------        | ---------     | ----              |
///    | Topic1         | Message1      | "topics/one"      |
///    | Topic2         | Message2      | "topics/two"      |
/// }
/// ```

#[macro_export]
macro_rules! topics {
    (
        list = $list_name:ident;
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

        pub const $list_name: $crate::TopicMap = $crate::TopicMap {
            types: &[
                $(
                    $(#[$meta])?
                    <$tp_name as $crate::Topic>::Message::SCHEMA,
                )*
            ],
            topics: &[
                $(
                    $(#[$meta])?
                    (
                        <$tp_name as $crate::Topic>::PATH,
                        <$tp_name as $crate::Topic>::TOPIC_KEY,
                    ),
                )*
            ],
        };
    };
}

// TODO: bring this back when I sort out how to do formatting in the sender!
// This might require WireTx impls
//
// #[cfg(feature = "embassy-usb-0_3-server")]
// #[macro_export]
// macro_rules! sender_log {
//     ($sender:ident, $($arg:tt)*) => {
//         $sender.fmt_publish::<$crate::standard_icd::Logging>(format_args!($($arg)*))
//     };
//     ($sender:ident, $s:expr) => {
//         $sender.str_publish::<$crate::standard_icd::Logging>($s)
//     };
//     ($($arg:tt)*) => {
//         compile_error!("You must pass the sender to `sender_log`!");
//     }
// }

#[cfg(test)]
mod endpoints_test {
    use postcard_schema::Schema;
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
        | TopicTy        | MessageTy     | Path              |
        | ----------     | ---------     | ----              |
        | BetaTopic1     | BTopic        | "test/beta1"      |
        | BetaTopic2     | BTopic        | "test/beta2"      |
        | BetaTopic3     | BTopic        | "test/beta3"      |
    }

    #[test]
    fn eps() {
        assert_eq!(ENDPOINT_LIST.types.len(), 6);
        assert_eq!(ENDPOINT_LIST.endpoints.len(), 3);
    }

    #[test]
    fn tps() {
        assert_eq!(TOPICS_IN_LIST.types.len(), 3);
        assert_eq!(TOPICS_IN_LIST.topics.len(), 3);
    }
}
