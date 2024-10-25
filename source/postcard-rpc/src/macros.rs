/// ## Endpoint macro
///
/// Used to define a single Endpoint marker type that implements the
/// [Endpoint][crate::Endpoint] trait.
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

#[macro_export]
macro_rules! endpoints {
    (
           list = $list_name:ident;
           | EndpointTy     | RequestTy     | ResponseTy    | Path              |
           | $(-)*          | $(-)*         | $(-)*         | $(-)*             |
        $( | $ep_name:ident | $req_ty:ty    | $resp_ty:ty   | $path_str:literal | )*
    ) => {
        // struct definitions and trait impls
        $(
            pub struct $ep_name;

            impl $crate::Endpoint for $ep_name {
                type Request = $req_ty;
                type Response = $resp_ty;
                const PATH: &'static str = $path_str;
                const REQ_KEY: $crate::Key = $crate::Key::for_path::<$req_ty>($path_str);
                const RESP_KEY: $crate::Key = $crate::Key::for_path::<$resp_ty>($path_str);
            }
        )*

        pub const $list_name: $crate::EndpointMap = $crate::EndpointMap {
            types: &[
                $(
                    <$ep_name as $crate::Endpoint>::Request::SCHEMA,
                    <$ep_name as $crate::Endpoint>::Response::SCHEMA,
                )*
            ],
            endpoints: &[
                $(
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
        pub struct $tyname;

        impl $crate::Topic for $tyname {
            type Message = $msg;
            const PATH: &'static str = $path;
            const TOPIC_KEY: $crate::Key = $crate::Key::for_path::<$msg>($path);
        }
    };
}

#[macro_export]
macro_rules! topics {
    (
           list = $list_name:ident;
           | TopicTy        | MessageTy     | Path              |
           | $(-)*          | $(-)*         | $(-)*             |
        $( | $tp_name:ident | $msg_ty:ty    | $path_str:literal | )*
    ) => {
        // struct definitions and trait impls
        $(
            pub struct $tp_name;

            impl $crate::Topic for $tp_name {
                type Message = $msg_ty;
                const PATH: &'static str = $path_str;
                const TOPIC_KEY: $crate::Key = $crate::Key::for_path::<$msg_ty>($path_str);
            }
        )*

        pub const $list_name: $crate::TopicMap = $crate::TopicMap {
            types: &[
                $(
                    <$tp_name as $crate::Topic>::Message::SCHEMA,
                )*
            ],
            topics: &[
                $(
                    (
                        <$tp_name as $crate::Topic>::PATH,
                        <$tp_name as $crate::Topic>::TOPIC_KEY,
                    ),
                )*
            ],
        };
    };
}

#[cfg(feature = "embassy-usb-0_3-server")]
#[macro_export]
macro_rules! sender_log {
    ($sender:ident, $($arg:tt)*) => {
        $sender.fmt_publish::<$crate::standard_icd::Logging>(format_args!($($arg)*))
    };
    ($sender:ident, $s:expr) => {
        $sender.str_publish::<$crate::standard_icd::Logging>($s)
    };
    ($($arg:tt)*) => {
        compile_error!("You must pass the sender to `sender_log`!");
    }
}

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
