/// Define Dispatch Macro
///
/// # Example
///
/// ```rust,ignore
/// use postcard_rpc::define_dispatch;
/// use postcard_rpc::server::impls::test_channels::dispatch_impl::*;
///
/// // This creates a type that implements the `Dispatcher` trait
/// define_dispatch! {
///     // This becomes the name of your dispatcher
///     app: SingleDispatcher;
///     // This is the spawn function, usually found in the `dispatch_impl` module of your
///     // implementation
///     spawn_fn: spawn_fn;
///     // This is the WireTx impl
///     tx_impl: WireTxImpl;
///     // This is the WireSpawn impl
///     spawn_impl: WireSpawnImpl;
///     // This is the TestContext you define to be passed to all handlers
///     context: TestContext;
///
///     endpoints: {
///         // This is the list you get from the `endpoints()` macro
///         list: ENDPOINT_LIST;
///
///         // These are all of your endpoints and the handlers they map to
///         | EndpointTy        | kind      | handler               |
///         | ----------        | ----      | -------               |
///         | AlphaEndpoint     | async     | test_alpha_handler    |
///         | BetaEndpoint      | spawn     | test_beta_handler     |
///     };
///     topics_in: {
///         // This is the list you get from the `topics!()` macro
///         list: TOPICS_IN_LIST;
///
///         // These are the incoming topics and the handlers they map to
///         | TopicTy           | kind      | handler               |
///         | ----------        | ----      | -------               |
///         | ZetaTopic1        | blocking  | test_zeta_blocking    |
///         | ZetaTopic2        | async     | test_zeta_async       |
///         | ZetaTopic3        | spawn     | test_zeta_spawn       |
///     };
///     topics_out: {
///         // This is the list you get from the `topics!()` macro
///         list: TOPICS_OUT_LIST;
///
///         // NOTE: outgoing topics don't have any handlers!
///     };
/// }
/// ```
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
    // One Key-width matcher. We generate this for 1, 2, 4, and 8 byte keys; only
    // the width selected by `sizer::NEEDED_SZ` is called from `Dispatch::handle`.
    //////////////////////////////////////////////////////////////////////////////
    (@matcher
        $handle_fn:ident $app_name:ident $tx_impl:ty; $spawn_fn:ident $key_ty:ty;
        $endpoint_key_name:ident / $topic_key_name:ident
        ($($endpoint:ty | $ep_flavor:tt | $ep_handler:ident)*)
        ($($topic_in:ty | $tp_flavor:tt | $tp_handler:ident)*)
    ) => {
        impl $app_name {
            async fn $handle_fn(
                &mut self,
                tx: &$crate::server::Sender<$tx_impl>,
                hdr: &$crate::header::VarHeader,
                body: &[u8],
            ) -> Result<(), <$tx_impl as $crate::server::WireTx>::Error> {
                let key = hdr.key;
                let Ok(keyb) = <$key_ty>::try_from(&key) else {
                    let err = $crate::standard_icd::WireError::KeyTooSmall;
                    return tx.error(hdr.seq_no, err).await;
                };

                // Store some items as named bindings, so we can use `ident` in the
                // recursive macro expansion. Load bearing order: we borrow `context`
                // from `dispatch` because we need `dispatch` AFTER `context`, so NLL
                // allows this to still borrowck
                let context = &mut self.context;
                let spawninfo = &self.spawn;

                match keyb {
                    // Standard ICD endpoints
                    //
                    // WARNING! If you add any more standard icd endpoints, make sure you ALSO add them
                    // to ALL_DISPATCH_KEYS in sizer!
                    <$crate::standard_icd::PingEndpoint as $crate::Endpoint>::$endpoint_key_name => {
                        // Can we deserialize the request?
                        let Ok(req) = $crate::postcard::from_bytes::<<$crate::standard_icd::PingEndpoint as $crate::Endpoint>::Request>(body) else {
                            let err = $crate::standard_icd::WireError::DeserFailed;
                            return tx.error(hdr.seq_no, err).await;
                        };

                        tx.reply::<$crate::standard_icd::PingEndpoint>(hdr.seq_no, &req).await
                    },
                    <$crate::standard_icd::GetAllSchemasEndpoint as $crate::Endpoint>::$endpoint_key_name => {
                        tx.send_all_schemas(hdr, self.device_map).await
                    }
                    // WARNING! If you add any more standard icd endpoints, make sure you ALSO add them
                    // to ALL_DISPATCH_KEYS in sizer!
                    //
                    // end standard_icd endpoints
                    $(
                        <$endpoint as $crate::Endpoint>::$endpoint_key_name => {
                            // Can we deserialize the request?
                            let Ok(req) = $crate::postcard::from_bytes::<<$endpoint as $crate::Endpoint>::Request>(body) else {
                                let err = $crate::standard_icd::WireError::DeserFailed;
                                return tx.error(hdr.seq_no, err).await;
                            };

                            // This will expand to the right "flavor" of handler
                            $crate::define_dispatch!(@ep_arm $ep_flavor ($endpoint) $ep_handler context hdr req tx ($spawn_fn) spawninfo)
                        }
                    )*
                    $(
                        <$topic_in as $crate::Topic>::$topic_key_name => {
                            // Can we deserialize the request?
                            let Ok(msg) = $crate::postcard::from_bytes::<<$topic_in as $crate::Topic>::Message>(body) else {
                                // This is a topic, not much to be done
                                return Ok(());
                            };

                            $crate::define_dispatch!(@tp_arm $tp_flavor $tp_handler context hdr msg tx ($spawn_fn) spawninfo);
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
            list: $endpoint_list:path;

               | EndpointTy     | kind          | handler           |
               | $(-)*          | $(-)*         | $(-)*             |
            $( | $endpoint:ty   | $ep_flavor:tt | $ep_handler:ident  | )*
        };
        topics_in: {
            list: $topic_in_list:path;

               | TopicTy        | kind          | handler           |
               | $(-)*          | $(-)*         | $(-)*             |
            $( | $topic_in:ty   | $tp_flavor:tt | $tp_handler:ident  | )*
        };
        topics_out: {
            list: $topic_out_list:path;
        };
    ) => {

        // Here, we calculate how many bytes (1, 2, 4, or 8) are required to uniquely
        // match on the given messages we receive and send.
        //
        // This serves as a sort of "perfect hash function", allowing us to use fewer
        // bytes on the wire.
        mod sizer {
            use super::*;
            use $crate::Key;

            const fn path_keys<const N: usize>(items: &[(&'static str, Key)]) -> [Key; N] {
                let mut keys = [unsafe { Key::from_bytes([0; 8]) }; N];
                let mut i = 0;
                while i < N {
                    keys[i] = items[i].1;
                    i += 1;
                }
                keys
            }

            // Create a list of JUST the ENDPOINT keys from the endpoint report
            const EP_KEYS: [Key; $endpoint_list.endpoints.len()] =
                path_keys($endpoint_list.endpoints);
            // Create a list of JUST the MESSAGE keys from the TOPICS IN report
            const TP_IN_KEYS: [Key; $topic_in_list.topics.len()] =
                path_keys($topic_in_list.topics);
            // Create a list of JUST the MESSAGE keys from the TOPICS OUT report
            const TP_OUT_KEYS: [Key; $topic_out_list.topics.len()] =
                path_keys($topic_out_list.topics);

            // This is a list of all ENDPOINT KEYS in the actual handlers
            //
            // This should be a SUBSET of the ENDPOINT KEYS in the Endpoint report
            const EP_HANDLER_KEYS: &[Key] = &[
                $(<$endpoint as $crate::Endpoint>::ENDPOINT_KEY,)*
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

            // Same keys the matcher will see, plus the standard ICD endpoints that
            // are always injected. Compared at NEEDED_SZ so we catch omit_std
            // collisions at the live wire width, not only identical Key8s.
            const ALL_DISPATCH_KEYS: &[Key] = &[
                <$crate::standard_icd::PingEndpoint as $crate::Endpoint>::ENDPOINT_KEY,
                <$crate::standard_icd::GetAllSchemasEndpoint as $crate::Endpoint>::ENDPOINT_KEY,
                $(<$endpoint as $crate::Endpoint>::ENDPOINT_KEY,)*
                $(<$topic_in as $crate::Topic>::TOPIC_KEY,)*
            ];

            const fn keys_match_at_width(a: Key, b: Key, n: usize) -> bool {
                match n {
                    1 => $crate::Key1::from_key8(a).const_cmp(&$crate::Key1::from_key8(b)),
                    2 => $crate::Key2::from_key8(a).const_cmp(&$crate::Key2::from_key8(b)),
                    4 => $crate::Key4::from_key8(a).const_cmp(&$crate::Key4::from_key8(b)),
                    8 => a.const_cmp(&b),
                    _ => unreachable!(),
                }
            }

            const fn has_dupe_at_width(keys: &[Key], n: usize) -> bool {
                let mut i = 0;
                while i < keys.len() {
                    let mut j = i + 1;
                    while j < keys.len() {
                        if keys_match_at_width(keys[i], keys[j], n) {
                            return true;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                false
            }

            pub const NEEDED_SZ_IN: usize = $crate::server::min_key_needed(&[
                &EP_KEYS,
                &TP_IN_KEYS,
            ]);
            pub const NEEDED_SZ_OUT: usize = $crate::server::min_key_needed(&[
                &EP_KEYS,
                &TP_OUT_KEYS,
            ]);
            pub const NEEDED_SZ: usize = const {
                assert!(
                    a_is_subset_of_b(EP_HANDLER_KEYS, &EP_KEYS),
                    "All listed endpoint handlers must be listed in endpoints->list! Missing Endpoint Type found!",
                );
                assert!(
                    a_is_subset_of_b(TP_HANDLER_IN_KEYS, &TP_IN_KEYS),
                    "All listed topic-in handlers must be listed in topics_in->list! Missing Topic Type found!",
                );
                if NEEDED_SZ_IN > NEEDED_SZ_OUT {
                    NEEDED_SZ_IN
                } else {
                    NEEDED_SZ_OUT
                }
            };

            // Check if there are any unexpected duplicates, typically this occurs
            // because the user has set `omit_std`
            pub const HAS_DUPE: bool = has_dupe_at_width(ALL_DISPATCH_KEYS, NEEDED_SZ);
        }

        // This is the fun part.
        //
        // For... reasons, we need to generate a match function to allow for dispatching
        // different async handlers without degrading to dyn Future, because no alloc on
        // embedded systems.
        //
        // Macros run before we know `NEEDED_SZ`, so we emit a matcher for each of
        // 1, 2, 4, and 8 byte keys. `Dispatch::handle` then calls only the one that
        // matches the const-computed width.
        #[doc=concat!("This defines the postcard-rpc app implementation for ", stringify!($app_name))]
        pub struct $app_name {
            pub context: $context_ty,
            pub spawn: $spawn_impl,
            pub device_map: &'static $crate::DeviceMap,
        }

        const _DUPE_CHECK: () = const {
            assert!(!sizer::HAS_DUPE, "Caught duplicate items. Is `omit_std` set? This is likely a bug in your code. See https://github.com/jamesmunns/postcard-rpc/issues/135.");
        };

        impl $app_name {
            /// Create a new instance of the dispatcher
            pub fn new(
                context: $context_ty,
                spawn: $spawn_impl,
            ) -> Self {
                const MAP: &$crate::DeviceMap = &$crate::DeviceMap {
                    types: const {
                        const LISTS: &[&[&'static $crate::postcard_schema::schema::NamedType]] = &[
                            $endpoint_list.types,
                            $topic_in_list.types,
                            $topic_out_list.types,
                        ];
                        const TTL_COUNT: usize = $endpoint_list.types.len() + $topic_in_list.types.len() + $topic_out_list.types.len();

                        const BIG_RPT: ([Option<&'static $crate::postcard_schema::schema::NamedType>; TTL_COUNT], usize) = $crate::uniques::merge_nty_lists(LISTS);
                        const SMALL_RPT: [&'static $crate::postcard_schema::schema::NamedType; BIG_RPT.1] = $crate::uniques::cruncher(BIG_RPT.0.as_slice());
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

        $crate::define_dispatch! {
            @matcher handle_key1 $app_name $tx_impl; $spawn_fn $crate::Key1;
            ENDPOINT_KEY1 / TOPIC_KEY1
            ($($endpoint | $ep_flavor | $ep_handler)*)
            ($($topic_in | $tp_flavor | $tp_handler)*)
        }
        $crate::define_dispatch! {
            @matcher handle_key2 $app_name $tx_impl; $spawn_fn $crate::Key2;
            ENDPOINT_KEY2 / TOPIC_KEY2
            ($($endpoint | $ep_flavor | $ep_handler)*)
            ($($topic_in | $tp_flavor | $tp_handler)*)
        }
        $crate::define_dispatch! {
            @matcher handle_key4 $app_name $tx_impl; $spawn_fn $crate::Key4;
            ENDPOINT_KEY4 / TOPIC_KEY4
            ($($endpoint | $ep_flavor | $ep_handler)*)
            ($($topic_in | $tp_flavor | $tp_handler)*)
        }
        $crate::define_dispatch! {
            @matcher handle_key8 $app_name $tx_impl; $spawn_fn $crate::Key;
            ENDPOINT_KEY / TOPIC_KEY
            ($($endpoint | $ep_flavor | $ep_handler)*)
            ($($topic_in | $tp_flavor | $tp_handler)*)
        }

        impl $crate::server::Dispatch for $app_name {
            type Tx = $tx_impl;

            fn min_key_len(&self) -> $crate::header::VarKeyKind {
                self.device_map.min_key_len
            }

            async fn handle(
                &mut self,
                tx: &$crate::server::Sender<Self::Tx>,
                hdr: &$crate::header::VarHeader,
                body: &[u8],
            ) -> Result<(), <Self::Tx as $crate::server::WireTx>::Error> {
                match sizer::NEEDED_SZ {
                    1 => self.handle_key1(tx, hdr, body).await,
                    2 => self.handle_key2(tx, hdr, body).await,
                    4 => self.handle_key4(tx, hdr, body).await,
                    8 => self.handle_key8(tx, hdr, body).await,
                    _ => unreachable!(),
                }
            }
        }

    }
}
