//! A basic postcard-rpc compatible application

use crate::{
    handlers::{get_led, set_led, sleep_handler, unique_id},
    icd::{
        GetLedEndpoint, GetUniqueIdEndpoint, SetLedEndpoint, SleepEndpoint, ENDPOINT_LIST,
        TOPICS_IN_LIST, TOPICS_OUT_LIST,
    },
};
use embassy_nrf::{
    buffered_uarte::{BufferedUarteRx, BufferedUarteTx},
    gpio::Output,
    peripherals::{TIMER0, UARTE0},
};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use postcard_rpc::server::impls::embedded_io_async_v0_6::{
    dispatch_impl::{spawn_fn, WireRxBuf, WireSpawnImpl},
    EioWireRx, EioWireTx, WireStorage,
};
use postcard_rpc::{
    define_dispatch,
    server::{Server, SpawnContext},
};

/// Context contains the data that we will pass (as a mutable reference)
/// to each endpoint or topic handler
pub struct Context {
    /// We'll use this unique ID to identify ourselves to the poststation
    /// server. This should be unique per device.
    pub unique_id: u64,
    pub led: Output<'static>,
}

impl SpawnContext for Context {
    type SpawnCtxt = TaskContext;

    fn spawn_ctxt(&mut self) -> Self::SpawnCtxt {
        TaskContext {
            unique_id: self.unique_id,
        }
    }
}

pub struct TaskContext {
    pub unique_id: u64,
}

// Type Aliases
//
// These aliases are used to keep the types from getting too out of hand.
//
// If you are using the nRF52840 - you shouldn't need to modify any of these!
//
// This alias describes the type of driver we will need. In this case, we
// are using the embassy-io driver with the nRF52840 UARTE0 peripheral

pub type Rx = BufferedUarteRx<'static, UARTE0, TIMER0>;
pub type Tx = BufferedUarteTx<'static, UARTE0>;
pub type Storage = WireStorage<Rx, Tx, ThreadModeRawMutex, 1024, 1024>;
pub type AppTx = EioWireTx<ThreadModeRawMutex, BufferedUarteTx<'static, UARTE0>>;

/// AppRx is the type of our receiver, which is how we receive information from the client
pub type AppRx = EioWireRx<Rx>;
/// AppServer is the type of the postcard-rpc server we are using
pub type AppServer = Server<AppTx, AppRx, WireRxBuf, MyApp>;

/// STORAGE
pub static STORAGE: Storage = Storage::new();

// This macro defines your application
define_dispatch! {
    // You can set the name of your app to any valid Rust type name. We use
    // "MyApp" here. You'll use this in `main` to create an instance of the
    // app.
    app: MyApp;
    // This chooses how we spawn functions. Here, we use the implementation
    // from the `embassy_usb_v0_4` implementation
    spawn_fn: spawn_fn;
    // This is our TX impl, which we aliased above
    tx_impl: AppTx;
    // This is our spawn impl, which also comes from `embassy_usb_v0_4`.
    spawn_impl: WireSpawnImpl;
    // This is the context type we defined above
    context: Context;

    // Endpoints are how we handle request/response pairs from the client.
    //
    // The "EndpointTy" are the names of the endpoints we defined in our ICD
    // crate. The "kind" is the kind of handler, which can be "blocking",
    // "async", or "spawn". Blocking endpoints will be called directly.
    // Async endpoints will also be called directly, but will be await-ed on,
    // allowing you to call async functions. Spawn endpoints will spawn an
    // embassy task, which allows for handling messages that may take some
    // amount of time to complete.
    //
    // The "handler"s are the names of the functions (or tasks) that will be
    // called when messages from this endpoint are received.
    endpoints: {
        // This list comes from our ICD crate. All of the endpoint handlers we
        // define below MUST be contained in this list.
        list: ENDPOINT_LIST;

        | EndpointTy                | kind      | handler                       |
        | ----------                | ----      | -------                       |
        | GetUniqueIdEndpoint       | blocking  | unique_id                     |
        | SleepEndpoint             | spawn     | sleep_handler                 |
        | SetLedEndpoint            | blocking  | set_led                       |
        | GetLedEndpoint            | blocking  | get_led                       |
    };

    // Topics IN are messages we receive from the client, but that we do not reply
    // directly to. These have the same "kinds" and "handlers" as endpoints, however
    // these handlers never return a value
    topics_in: {
        // This list comes from our ICD crate. All of the topic handlers we
        // define below MUST be contained in this list.
        list: TOPICS_IN_LIST;

        | TopicTy                   | kind      | handler                       |
        | ----------                | ----      | -------                       |
    };

    // Topics OUT are the messages we send to the client whenever we'd like. Since
    // these are outgoing, we do not need to define handlers for them.
    topics_out: {
        // This list comes from our ICD crate.
        list: TOPICS_OUT_LIST;
    };
}
