/// # Define Dispatch Macro
///

#[macro_export]
macro_rules! define_dispatch {
    //////////////////////////////////////////////////////////////////////////////
    // ENDPOINT HANDLER EXPANSION ARMS
    //////////////////////////////////////////////////////////////////////////////

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
            let context = $crate::server::SpawnContext::spawn_ctxt($context);
            if $spawn_fn($spawner, $handler(context, $header.clone(), $req, $outputter.clone())).is_err() {
                let err = $crate::standard_icd::WireError::FailedToSpawn;
                $outputter.error($header.seq_no, err).await
            } else {
                Ok(())
            }
        }
    };

    //////////////////////////////////////////////////////////////////////////////
    // TOPIC HANDLER EXPANSION ARMS
    //////////////////////////////////////////////////////////////////////////////

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
            let context = $crate::server::SpawnContext::spawn_ctxt($context);
            let _ = $spawn_fn($spawner, $handler(context, $header.clone(), $msg, $outputter.clone()));
        }
    };

    //////////////////////////////////////////////////////////////////////////////
    // Implementation of the dispatch trait for the app, where the Key length
    // is N, where N is 1, 2, 4, or 8
    //////////////////////////////////////////////////////////////////////////////
    (@matcher
        $n:literal $app_name:ident $tx_impl:ty; $spawn_fn:ident $key_ty:ty; $key_kind:expr;
        ($($endpoint:ty | $ep_flavor:tt | $ep_handler:ident)*)
        ($($topic_in:ty | $tp_flavor:tt | $tp_handler:ident)*)
    ) => {
        impl $crate::server::Dispatch2 for $app_name<$n> {
            type Tx = $tx_impl;

            fn min_key_len(&self) -> $crate::header::VarKeyKind {
                $key_kind
            }

            /// Handle dispatching of a single frame
            async fn handle(
                &mut self,
                tx: &$crate::server::Sender<Self::Tx>,
                hdr: &$crate::header::VarHeader,
                body: &[u8],
            ) -> Result<(), <Self::Tx as $crate::server::WireTx>::Error> {
                let key = hdr.key;
                let Some(keyb) = <$key_ty>::try_from_varkey(&key) else {
                    let err = $crate::standard_icd::WireError::KeyTooSmall;
                    return tx.error(hdr.seq_no, err).await;
                };
                let keyb = keyb.to_bytes();
                use consts::*;
                match keyb {
                    $(
                        ::paste::paste! { [<$endpoint:upper _KEY $n>] } => {
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
                            define_dispatch!(@ep_arm $ep_flavor ($endpoint) $ep_handler context hdr req tx ($spawn_fn) spawninfo)
                        }
                    )*
                    $(
                        ::paste::paste! { [<$topic_in:upper _KEY $n>] } => {
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
                            define_dispatch!(@tp_arm $tp_flavor $tp_handler context hdr msg tx ($spawn_fn) spawninfo);
                            Ok(())
                        }
                    )*
                    _other => {
                        // huh! We have no idea what this key is supposed to be!
                        let err = $crate::standard_icd::WireError::UnknownKey;
                        tx.error(hdr.seq_no, err).await
                    },
                }
            }
        }
    };

    //////////////////////////////////////////////////////////////////////////////
    // MAIN EXPANSION ENTRYPOINT
    //////////////////////////////////////////////////////////////////////////////
    (
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
        topics_out: {
            list: $topic_out_list:ident;
        };
    ) => {

        // Here, we calculate how many bytes (1, 2, 4, or 8) are required to uniquely
        // match on the given messages we receive and send†.
        //
        // This serves as a sort of "perfect hash function", allowing us to use fewer
        // bytes on the wire.
        //
        // †: We don't calculate sending keys yet, oops. This probably requires hashing
        // TX/RX differently so endpoints with the same TX and RX don't collide, or
        // calculating them separately and taking the max
        mod sizer {
            use super::*;
            use $crate::Key;

            // TODO: Warn/error if the list doesn't match the defined handlers?

            const KEY_SLI_IN: &[Key] = &[
                $(<$endpoint as $crate::Endpoint>::REQ_KEY,)*
                $(<$topic_in as $crate::Topic>::TOPIC_KEY,)*
            ];
            const KEYS_IN: [Key; KEY_SLI_IN.len()] = [
                $(<$endpoint as $crate::Endpoint>::REQ_KEY,)*
                $(<$topic_in as $crate::Topic>::TOPIC_KEY,)*
            ];
            pub const NEEDED_SZ_IN: usize = $crate::server::min_key_needed(&[&KEYS_IN]);
        }


        // Here, we calculate at const time the keys we need to match against. This is done with
        // paste, which is unfortunate, but allows us to match on this correctly later.
        mod consts {
            use super::*;
            $(
                ::paste::paste! {
                    pub const [<$endpoint:upper _KEY1>]: u8 = $crate::Key1::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$topic_in:upper _KEY1>]: u8 = $crate::Key1::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$endpoint:upper _KEY2>]: [u8; 2] = $crate::Key2::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$topic_in:upper _KEY2>]: [u8; 2] = $crate::Key2::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$endpoint:upper _KEY4>]: [u8; 4] = $crate::Key4::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$topic_in:upper _KEY4>]: [u8; 4] = $crate::Key4::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$endpoint:upper _KEY8>]: [u8; 8] = <$endpoint as $crate::Endpoint>::REQ_KEY.to_bytes();
                }
            )*
            $(
                ::paste::paste! {
                    pub const [<$topic_in:upper _KEY8>]: [u8; 8] = <$topic_in as $crate::Topic>::TOPIC_KEY.to_bytes();
                }
            )*
        }

        // This is the fun part.
        //
        // For... reasons, we need to generate a match function to allow for dispatching
        // different async handlers without degrading to dyn Future, because no alloc on
        // embedded systems.
        //
        // The easiest way I've found to achieve this is actually to implement this
        // handler for ALL of 1, 2, 4, 8, BUT to hide that from the user, and instead
        // use THIS alias to give them the one that they need.
        //
        // This is overly complicated because I'm mixing const-time capabilities with
        // macro-time capabilities. I'm very open to other suggestions that achieve the
        // same outcome.
        pub type $app_name = impls::$app_name<{ sizer::NEEDED_SZ_IN }>;

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

            define_dispatch! {
                @matcher 1 $app_name $tx_impl; $spawn_fn $crate::Key1; $crate::header::VarKeyKind::Key1;
                ($($endpoint | $ep_flavor | $ep_handler)*)
                ($($topic_in | $tp_flavor | $tp_handler)*)
            }
            define_dispatch! {
                @matcher 2 $app_name $tx_impl; $spawn_fn $crate::Key2; $crate::header::VarKeyKind::Key2;
                ($($endpoint | $ep_flavor | $ep_handler)*)
                ($($topic_in | $tp_flavor | $tp_handler)*)
            }
            define_dispatch! {
                @matcher 4 $app_name $tx_impl; $spawn_fn $crate::Key4; $crate::header::VarKeyKind::Key4;
                ($($endpoint | $ep_flavor | $ep_handler)*)
                ($($topic_in | $tp_flavor | $tp_handler)*)
            }
            define_dispatch! {
                @matcher 8 $app_name $tx_impl; $spawn_fn $crate::Key; $crate::header::VarKeyKind::Key8;
                ($($endpoint | $ep_flavor | $ep_handler)*)
                ($($topic_in | $tp_flavor | $tp_handler)*)
            }
        }

    }
}
