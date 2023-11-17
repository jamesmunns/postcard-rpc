/// ## Endpoint macro
///
/// Used to define a single Endpoint marker type that implements the
/// [Endpoint][crate::Endpoint] trait.
///
/// ```rust
/// # use postcard::experimental::schema::Schema;
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
#[macro_export]
macro_rules! endpoint {
    ($tyname:ident, $req:ty, $resp:ty, $path:literal) => {
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
