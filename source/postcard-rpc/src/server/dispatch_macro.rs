/// # Define Dispatch Macro
///

#[doc(hidden)]
pub mod export {
    pub use paste::paste;
}

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
        impl $crate::server::Dispatch for $app_name<$n> {
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
                        $crate::server::dispatch_macro::export::paste! { [<$endpoint:upper _KEY $n>] } => {
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
                        $crate::server::dispatch_macro::export::paste! { [<$topic_in:upper _KEY $n>] } => {
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

            // Create a list of JUST the REQUEST keys from the endpoint report
            const EP_IN_KEYS_SZ: usize = $endpoint_list.endpoints.len();
            const EP_IN_KEYS: [Key; EP_IN_KEYS_SZ] = const {
                let mut keys = [unsafe { Key::from_bytes([0; 8]) }; EP_IN_KEYS_SZ];
                let mut i = 0;
                while i < EP_IN_KEYS_SZ {
                    keys[i] = $endpoint_list.endpoints[i].1;
                    i += 1;
                }
                keys
            };
            // Create a list of JUST the RESPONSE keys from the endpoint report
            const EP_OUT_KEYS_SZ: usize = $endpoint_list.endpoints.len();
            const EP_OUT_KEYS: [Key; EP_OUT_KEYS_SZ] = const {
                let mut keys = [unsafe { Key::from_bytes([0; 8]) }; EP_OUT_KEYS_SZ];
                let mut i = 0;
                while i < EP_OUT_KEYS_SZ {
                    keys[i] = $endpoint_list.endpoints[i].2;
                    i += 1;
                }
                keys
            };
            // Create a list of JUST the MESSAGE keys from the TOPICS IN report
            const TP_IN_KEYS_SZ: usize = $topic_in_list.topics.len();
            const TP_IN_KEYS: [Key; TP_IN_KEYS_SZ] = const {
                let mut keys = [unsafe { Key::from_bytes([0; 8]) }; TP_IN_KEYS_SZ];
                let mut i = 0;
                while i < TP_IN_KEYS_SZ {
                    keys[i] = $topic_in_list.topics[i].1;
                    i += 1;
                }
                keys
            };
            // Create a list of JUST the MESSAGE keys from the TOPICS OUT report
            const TP_OUT_KEYS_SZ: usize = $topic_out_list.topics.len();
            const TP_OUT_KEYS: [Key; TP_OUT_KEYS_SZ] = const {
                let mut keys = [unsafe { Key::from_bytes([0; 8]) }; TP_OUT_KEYS_SZ];
                let mut i = 0;
                while i < TP_OUT_KEYS_SZ {
                    keys[i] = $topic_out_list.topics[i].1;
                    i += 1;
                }
                keys
            };

            // This is a list of all REQUEST KEYS in the actual handlers
            //
            // This should be a SUBSET of the REQUEST KEYS in the Endpoint report
            const EP_HANDLER_IN_KEYS: &[Key] = &[
                $(<$endpoint as $crate::Endpoint>::REQ_KEY,)*
            ];
            // This is a list of all RESPONSE KEYS in the actual handlers
            //
            // This should be a SUBSET of the RESPONSE KEYS in the Endpoint report
            const EP_HANDLER_OUT_KEYS: &[Key] = &[
                $(<$endpoint as $crate::Endpoint>::RESP_KEY,)*
            ];
            // This is a list of all TOPIC KEYS in the actual handlers
            //
            // This should be a SUBSET of the TOPIC KEYS in the Topic IN report
            // (we can't check the out, we have no way of enumerating that yet,
            // which would require linkme-like crimes I think)
            const TP_HANDLER_IN_KEYS: &[Key] = &[
                $(<$topic_in as $crate::Topic>::TOPIC_KEY,)*
            ];

            const fn a_is_subset_of_b(a: &[Key], b: &[Key]) -> bool {
                let mut i = 0;
                while i < a.len() {
                    let x = u64::from_le_bytes(a[i].to_bytes());
                    let mut matched = false;
                    let mut j = 0;
                    while j < b.len() {
                        let y = u64::from_le_bytes(b[j].to_bytes());
                        if x == y {
                            matched = true;
                            break;
                        }
                        j += 1;
                    }
                    if !matched {
                        return false;
                    }
                    i += 1;
                }
                true
            }

            // TODO: Warn/error if the list doesn't match the defined handlers?
            pub const NEEDED_SZ_IN: usize = $crate::server::min_key_needed(&[
                &EP_IN_KEYS,
                &TP_IN_KEYS,
            ]);
            pub const NEEDED_SZ_OUT: usize = $crate::server::min_key_needed(&[
                &EP_OUT_KEYS,
                &TP_OUT_KEYS,
            ]);
            pub const NEEDED_SZ: usize = const {
                assert!(
                    a_is_subset_of_b(EP_HANDLER_IN_KEYS, &EP_IN_KEYS),
                    "All listed endpoint handlers must be listed in endpoints->list! Missing Requst Type found!",
                );
                assert!(
                    a_is_subset_of_b(EP_HANDLER_OUT_KEYS, &EP_OUT_KEYS),
                    "All listed endpoint handlers must be listed in endpoints->list! Missing Response Type found!",
                );
                assert!(
                    a_is_subset_of_b(TP_HANDLER_IN_KEYS, &TP_IN_KEYS),
                    "All listed endpoint handlers must be listed in endpoints->list! Missing Response Type found!",
                );
                if NEEDED_SZ_IN > NEEDED_SZ_OUT {
                    NEEDED_SZ_IN
                } else {
                    NEEDED_SZ_OUT
                }
            };
        }


        // Here, we calculate at const time the keys we need to match against. This is done with
        // paste, which is unfortunate, but allows us to match on this correctly later.
        mod consts {
            use super::*;
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$endpoint:upper _KEY1>]: u8 = $crate::Key1::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$topic_in:upper _KEY1>]: u8 = $crate::Key1::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$endpoint:upper _KEY2>]: [u8; 2] = $crate::Key2::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$topic_in:upper _KEY2>]: [u8; 2] = $crate::Key2::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$endpoint:upper _KEY4>]: [u8; 4] = $crate::Key4::from_key8(<$endpoint as $crate::Endpoint>::REQ_KEY).to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$topic_in:upper _KEY4>]: [u8; 4] = $crate::Key4::from_key8(<$topic_in as $crate::Topic>::TOPIC_KEY).to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
                    pub const [<$endpoint:upper _KEY8>]: [u8; 8] = <$endpoint as $crate::Endpoint>::REQ_KEY.to_bytes();
                }
            )*
            $(
                $crate::server::dispatch_macro::export::paste! {
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
        pub type $app_name = impls::$app_name<{ sizer::NEEDED_SZ }>;



        mod impls {
            use super::*;

            pub struct $app_name<const N: usize> {
                pub context: $context_ty,
                pub spawn: $spawn_impl,
                pub device_map: &'static $crate::DeviceMap,
            }

            impl<const N: usize> $app_name<N> {
                /// Create a new instance of the dispatcher
                pub fn new(
                    context: $context_ty,
                    spawn: $spawn_impl,
                ) -> Self {
                    const MAP: &$crate::DeviceMap = &$crate::DeviceMap {
                        types: const {
                            const LISTS: &[&[&'static postcard_schema::schema::NamedType]] = &[
                                $endpoint_list.types,
                                $topic_in_list.types,
                                $topic_out_list.types,
                            ];
                            const TTL_COUNT: usize = $endpoint_list.types.len() + $topic_in_list.types.len() + $topic_out_list.types.len();

                            const BIG_RPT: ([Option<&'static postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(LISTS);
                            const SMALL_RPT: [&'static postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
                            SMALL_RPT.as_slice()
                        },
                        endpoints: &$endpoint_list.endpoints,
                        topics_in: &$topic_in_list.topics,
                        topics_out: &$topic_out_list.topics,
                        min_key_len: const {
                            match sizer::NEEDED_SZ {
                                1 => $crate::header::VarKeyKind::Key1,
                                2 => $crate::header::VarKeyKind::Key2,
                                4 => $crate::header::VarKeyKind::Key4,
                                8 => $crate::header::VarKeyKind::Key8,
                                _ => unreachable!(),
                            }
                        }
                    };
                    $app_name {
                        context,
                        spawn,
                        device_map: MAP,
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
