/// # Define Dispatch Macro
///
// ```rust,skip
// # use postcard_rpc::target_server::dispatch_macro::fake::*;
// # use postcard_rpc::{endpoint, target_server::{sender::Sender, SpawnContext}, WireHeader, define_dispatch};
// # use postcard_schema::Schema;
// # use embassy_usb_driver::{Bus, ControlPipe, EndpointIn, EndpointOut};
// # use serde::{Deserialize, Serialize};
//
// pub struct DispatchCtx;
// pub struct SpawnCtx;
//
// // This trait impl is necessary if you want to use the `spawn` variant,
// // as spawned tasks must take ownership of any context they need.
// impl SpawnContext for DispatchCtx {
//     type SpawnCtxt = SpawnCtx;
//     fn spawn_ctxt(&mut self) -> Self::SpawnCtxt {
//         SpawnCtx
//     }
// }
//
// define_dispatch2! {
//     dispatcher: Dispatcher<
//         Mutex = FakeMutex,
//         Driver = FakeDriver,
//         Context = DispatchCtx,
//     >;
//     AlphaEndpoint => async alpha_handler,
//     BetaEndpoint => async beta_handler,
//     GammaEndpoint => async gamma_handler,
//     DeltaEndpoint => blocking delta_handler,
//     EpsilonEndpoint => spawn epsilon_handler_task,
// }
//
// async fn alpha_handler(_c: &mut DispatchCtx, _h: WireHeader, _b: AReq) -> AResp {
//     todo!()
// }
//
// async fn beta_handler(_c: &mut DispatchCtx, _h: WireHeader, _b: BReq) -> BResp {
//     todo!()
// }
//
// async fn gamma_handler(_c: &mut DispatchCtx, _h: WireHeader, _b: GReq) -> GResp {
//     todo!()
// }
//
// fn delta_handler(_c: &mut DispatchCtx, _h: WireHeader, _b: DReq) -> DResp {
//     todo!()
// }
//
// #[embassy_executor::task]
// async fn epsilon_handler_task(_c: SpawnCtx, _h: WireHeader, _b: EReq, _sender: Sender<FakeMutex, FakeDriver>) {
//     todo!()
// }
// ```

#[macro_export]
macro_rules! define_dispatch2 {
    // This is the "blocking execution" arm for defining an endpoint
    (@ep_arm blocking ($endpoint:ty) $handler:ident $context:ident $header:ident $req:ident $outputter:ident ($spawn_fn:path) $spawner:ident) => {
        {
            let reply = $handler($context, $header.clone(), $req);
            if $outputter.reply::<$endpoint>($header.seq_no, &reply).await.is_err() {
                let err = $crate::standard_icd::WireError::SerFailed;
                $outputter.error($header.seq_no, err).await
            } else {
                Ok(())
            }
        }
    };
    // This is the "async execution" arm for defining an endpoint
    (@ep_arm async ($endpoint:ty) $handler:ident $context:ident $header:ident $req:ident $outputter:ident ($spawn_fn:path) $spawner:ident) => {
        {
            let reply = $handler($context, $header.clone(), $req).await;
            if $outputter.reply::<$endpoint>($header.seq_no, &reply).await.is_err() {
                let err = $crate::standard_icd::WireError::SerFailed;
                $outputter.error($header.seq_no, err).await
            } else {
                Ok(())
            }
        }
    };
    // This is the "spawn an embassy task" arm for defining an endpoint
    (@ep_arm spawn ($endpoint:ty) $handler:ident $context:ident $header:ident $req:ident $outputter:ident ($spawn_fn:path) $spawner:ident) => {
        {
            let context = $crate::server2::SpawnContext::spawn_ctxt($context);
            if $spawn_fn($spawner, $handler(context, $header.clone(), $req, $outputter.clone())).is_err() {
                let err = $crate::standard_icd::WireError::FailedToSpawn;
                $outputter.error($header.seq_no, err).await
            } else {
                Ok(())
            }
        }
    };
    // This is the "blocking execution" arm for defining a topic
    (@tp_arm blocking $handler:ident $context:ident $header:ident $msg:ident $outputter:ident ($spawn_fn:path) $spawner:ident) => {
        {
            $handler($context, $header.clone(), $msg, $outputter);
        }
    };
    // This is the "async execution" arm for defining a topic
    (@tp_arm async $handler:ident $context:ident $header:ident $msg:ident $outputter:ident ($spawn_fn:path) $spawner:ident) => {
        {
            $handler($context, $header.clone(), $msg, $outputter).await;
        }
    };
    (@tp_arm spawn $handler:ident $context:ident $header:ident $msg:ident $outputter:ident ($spawn_fn:path) $spawner:ident) => {
        {
            let context = $crate::server2::SpawnContext::spawn_ctxt($context);
            let _ = $spawn_fn($spawner, $handler(context, $header.clone(), $msg, $outputter.clone()));
        }
    };

    (
        // dispatcher: $name:ident<WireTx = $wire_tx:ty, WireSpawn = $wire_spawn:ty, Context = $context:ty>;
        app: $app_name:ident;

        spawn_fn: $spawn_fn:ident;
        tx_impl: $tx_impl:ty;
        spawn_impl: $spawn_impl:ty;
        context: $context_ty:ty;

        endpoints: {
            list: $endpoint_list:ident;

               | EndpointTy     | kind          | handler           |
               | $(-)*          | $(-)*         | $(-)*             |
            $( | $endpoint:ty   | $ep_flavor:tt | $ep_handler:ident | )*
        };
        topics_in: {
            list: $topic_in_list:ident;

               | TopicTy        | kind          | handler           |
               | $(-)*          | $(-)*         | $(-)*             |
            $( | $topic_in:ty   | $tp_flavor:tt | $tp_handler:ident | )*
        };
    ) => {
        /// This is a structure that handles dispatching, generated by the
        /// `postcard-rpc::define_dispatch2!()` macro.
        mod sizer {
            use super::*;
            use $crate::Key;

            const KEY_SLI: &[Key] = &[
                $(<$endpoint as $crate::Endpoint>::REQ_KEY,)*
                $(<$topic_in as $crate::Topic>::TOPIC_KEY,)*
                // TODO: include out keys!
            ];
            const KEYS: [Key; KEY_SLI.len()] = [
                $(<$endpoint as $crate::Endpoint>::REQ_KEY,)*
                $(<$topic_in as $crate::Topic>::TOPIC_KEY,)*
                // TODO: include out keys!
            ];
            pub const NEEDED_SZ: usize = $crate::server2::min_key_needed(&KEYS);
        }
        pub type $app_name = impls::$app_name<{ sizer::NEEDED_SZ }>;

        mod consts {
            use super::*;
            $(
                paste::paste! {
                    pub const [<$endpoint:upper _KEY1>]: u8 = $crate::Key1::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).0;
                }
            )*
            $(
                paste::paste! {
                    pub const [<$topic_in:upper _KEY1>]: u8 = $crate::Key1::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).0;
                }
            )*
            $(
                paste::paste! {
                    pub const [<$endpoint:upper _KEY2>]: [u8; 2] = $crate::Key2::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).0;
                }
            )*
            $(
                paste::paste! {
                    pub const [<$topic_in:upper _KEY2>]: [u8; 2] = $crate::Key2::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).0;
                }
            )*
            $(
                paste::paste! {
                    pub const [<$endpoint:upper _KEY4>]: [u8; 4] = $crate::Key4::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).0;
                }
            )*
            $(
                paste::paste! {
                    pub const [<$topic_in:upper _KEY4>]: [u8; 4] = $crate::Key4::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).0;
                }
            )*
        }

        mod impls {
            use super::*;

            pub struct $app_name<const N: usize> {
                pub context: $context_ty,
                pub spawn: $spawn_impl,
                pub endpoint_list: &'static $crate::EndpointMap,
                pub topic_in_list: &'static $crate::TopicMap,
            }

            impl<const N: usize> $app_name<N> {
                /// Create a new instance of the dispatcher
                pub fn new(
                    context: $context_ty,
                    spawn: $spawn_impl,
                ) -> Self {
                    $app_name {
                        context,
                        spawn,
                        endpoint_list: &$endpoint_list,
                        topic_in_list: &$topic_in_list,
                    }
                }
            }

            //
            // 1
            //
            impl $crate::server2::Dispatch2 for $app_name<1> {
                type Tx = $tx_impl;

                /// Handle dispatching of a single frame
                async fn handle(
                    &mut self,
                    tx: &$crate::server2::Sender<Self::Tx>,
                    hdr: &$crate::WireHeader,
                    body: &[u8],
                ) -> Result<(), <Self::Tx as $crate::server2::WireTx>::Error> {
                    let keyb = $crate::Key1::from_key8(hdr.key).0;
                    use consts::*;
                    match keyb {
                        $(
                            paste::paste! { [<$endpoint:upper _KEY1>] } => {
                                // Can we deserialize the request?
                                let Ok(req) = postcard::from_bytes::<<$endpoint as $crate::Endpoint>::Request>(body) else {
                                    let err = $crate::standard_icd::WireError::DeserFailed;
                                    return tx.error(hdr.seq_no, err).await;
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // This will expand to the right "flavor" of handler
                                define_dispatch2!(@ep_arm $ep_flavor ($endpoint) $ep_handler context hdr req tx ($spawn_fn) spawninfo)
                            }
                        )*
                        $(
                            paste::paste! { [<$topic_in:upper _KEY1>] } => {
                                // Can we deserialize the request?
                                let Ok(msg) = postcard::from_bytes::<<$topic_in as $crate::Topic>::Message>(body) else {
                                    // This is a topic, not much to be done
                                    return Ok(());
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // (@tp_arm async $handler:ident $context:ident $header:ident $req:ident $outputter:ident)
                                define_dispatch2!(@tp_arm $tp_flavor $tp_handler context hdr msg tx ($spawn_fn) spawninfo);
                                Ok(())
                            }
                        )*
                        _other => {
                            // huh! We have no idea what this key is supposed to be!
                            let err = $crate::standard_icd::WireError::UnknownKey(hdr.key.to_bytes());
                            tx.error(hdr.seq_no, err).await
                        },
                    }
                }
            }

            //
            // 2
            //
            impl $crate::server2::Dispatch2 for $app_name<2> {
                type Tx = $tx_impl;

                /// Handle dispatching of a single frame
                async fn handle(
                    &mut self,
                    tx: &$crate::server2::Sender<Self::Tx>,
                    hdr: &$crate::WireHeader,
                    body: &[u8],
                ) -> Result<(), <Self::Tx as $crate::server2::WireTx>::Error> {
                    let keyb = $crate::Key2::from_key8(hdr.key).0;
                    use consts::*;
                    match keyb {
                        $(
                            paste::paste! { [<$endpoint:upper _KEY2>] } => {
                                // Can we deserialize the request?
                                let Ok(req) = postcard::from_bytes::<<$endpoint as $crate::Endpoint>::Request>(body) else {
                                    let err = $crate::standard_icd::WireError::DeserFailed;
                                    return tx.error(hdr.seq_no, err).await;
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // This will expand to the right "flavor" of handler
                                define_dispatch2!(@ep_arm $ep_flavor ($endpoint) $ep_handler context hdr req tx ($spawn_fn) spawninfo)
                            }
                        )*
                        $(
                            paste::paste! { [<$topic_in:upper _KEY2>] } => {
                                // Can we deserialize the request?
                                let Ok(msg) = postcard::from_bytes::<<$topic_in as $crate::Topic>::Message>(body) else {
                                    // This is a topic, not much to be done
                                    return Ok(());
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // (@tp_arm async $handler:ident $context:ident $header:ident $req:ident $outputter:ident)
                                define_dispatch2!(@tp_arm $tp_flavor $tp_handler context hdr msg tx ($spawn_fn) spawninfo);
                                Ok(())
                            }
                        )*
                        _other => {
                            // huh! We have no idea what this key is supposed to be!
                            let err = $crate::standard_icd::WireError::UnknownKey(hdr.key.to_bytes());
                            tx.error(hdr.seq_no, err).await
                        },
                    }
                }
            }
            //


            //
            // 4
            //
            impl $crate::server2::Dispatch2 for $app_name<4> {
                type Tx = $tx_impl;

                /// Handle dispatching of a single frame
                async fn handle(
                    &mut self,
                    tx: &$crate::server2::Sender<Self::Tx>,
                    hdr: &$crate::WireHeader,
                    body: &[u8],
                ) -> Result<(), <Self::Tx as $crate::server2::WireTx>::Error> {
                    let keyb = $crate::Key4::from_key8(hdr.key).0;
                    use consts::*;
                    match keyb {
                        $(
                            paste::paste! { [<$endpoint:upper _KEY4>] } => {
                                // Can we deserialize the request?
                                let Ok(req) = postcard::from_bytes::<<$endpoint as $crate::Endpoint>::Request>(body) else {
                                    let err = $crate::standard_icd::WireError::DeserFailed;
                                    return tx.error(hdr.seq_no, err).await;
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // This will expand to the right "flavor" of handler
                                define_dispatch2!(@ep_arm $ep_flavor ($endpoint) $ep_handler context hdr req tx ($spawn_fn) spawninfo)
                            }
                        )*
                        $(
                            paste::paste! { [<$topic_in:upper _KEY4>] } => {
                                // Can we deserialize the request?
                                let Ok(msg) = postcard::from_bytes::<<$topic_in as $crate::Topic>::Message>(body) else {
                                    // This is a topic, not much to be done
                                    return Ok(());
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // (@tp_arm async $handler:ident $context:ident $header:ident $req:ident $outputter:ident)
                                define_dispatch2!(@tp_arm $tp_flavor $tp_handler context hdr msg tx ($spawn_fn) spawninfo);
                                Ok(())
                            }
                        )*
                        _other => {
                            // huh! We have no idea what this key is supposed to be!
                            let err = $crate::standard_icd::WireError::UnknownKey(hdr.key.to_bytes());
                            tx.error(hdr.seq_no, err).await
                        },
                    }
                }
            }
            //


            //
            // 8
            //
            impl $crate::server2::Dispatch2 for $app_name<8> {
                type Tx = $tx_impl;

                /// Handle dispatching of a single frame
                async fn handle(
                    &mut self,
                    tx: &$crate::server2::Sender<Self::Tx>,
                    hdr: &$crate::WireHeader,
                    body: &[u8],
                ) -> Result<(), <Self::Tx as $crate::server2::WireTx>::Error> {
                    match hdr.key {
                        $(
                            <$endpoint as $crate::Endpoint>::REQ_KEY => {
                                // Can we deserialize the request?
                                let Ok(req) = postcard::from_bytes::<<$endpoint as $crate::Endpoint>::Request>(body) else {
                                    let err = $crate::standard_icd::WireError::DeserFailed;
                                    return tx.error(hdr.seq_no, err).await;
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // This will expand to the right "flavor" of handler
                                define_dispatch2!(@ep_arm $ep_flavor ($endpoint) $ep_handler context hdr req tx ($spawn_fn) spawninfo)
                            }
                        )*
                        $(
                            <$topic_in as $crate::Topic>::TOPIC_KEY => {
                                // Can we deserialize the request?
                                let Ok(msg) = postcard::from_bytes::<<$topic_in as $crate::Topic>::Message>(body) else {
                                    // This is a topic, not much to be done
                                    return Ok(());
                                };

                                // Store some items as named bindings, so we can use `ident` in the
                                // recursive macro expansion. Load bearing order: we borrow `context`
                                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                                // allows this to still borrowck
                                let dispatch = self;
                                let context = &mut dispatch.context;
                                #[allow(unused)]
                                let spawninfo = &dispatch.spawn;

                                // (@tp_arm async $handler:ident $context:ident $header:ident $req:ident $outputter:ident)
                                define_dispatch2!(@tp_arm $tp_flavor $tp_handler context hdr msg tx ($spawn_fn) spawninfo);
                                Ok(())
                            }
                        )*
                        _other => {
                            // huh! We have no idea what this key is supposed to be!
                            let err = $crate::standard_icd::WireError::UnknownKey(hdr.key.to_bytes());
                            tx.error(hdr.seq_no, err).await
                        },
                    }
                }
            }
            //
        }

    }
}
