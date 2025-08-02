//! A basic postcard-rpc compatible application

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_hal::Async;
use esp_hal::rmt::{ConstChannelAccess, Tx as RmtTx};
use esp_hal::usb_serial_jtag::{UsbSerialJtagRx, UsbSerialJtagTx};
use panic_rtt_target as _;

use esp_hal_smartled::{SmartLedsAdapter, smart_led_buffer};
use postcard_rpc::server::SpawnContext;
use postcard_rpc::server::impls::embedded_io_async_v0_6::WireStorage;
use smart_leds::RGB8;

use postcard_rpc::{
    define_dispatch,
    server::{
        Server,
        impls::embedded_io_async_v0_6::{
            EioWireTx,
            dispatch_impl::{WireRxBuf, WireRxImpl, WireSpawnImpl},
        },
    },
};

use workbook_icd::{
    ENDPOINT_LIST, GetUniqueIdEndpoint, PingEndpoint, SetAllLedEndpoint, SetSingleLedEndpoint,
    TOPICS_IN_LIST, TOPICS_OUT_LIST,
};

use crate::handlers::{ping_handler, set_all_led_handler, set_led_handler, unique_id_handler};

// Describe the single smartled on the ESP32-C6-DevKitC-1 board.
pub const LED_COUNT: usize = 1;
pub const LED_BUFFER_SIZE: usize = const { smart_led_buffer!(1).len() };

/// Context contains the data that we will pass (as a mutable reference)
/// to each endpoint or topic handler
pub struct Context {
    /// We'll use this unique ID to identify ourselves to the poststation
    /// server. This should be unique per device.
    pub unique_id: u64,
    pub led: SmartLedsAdapter<ConstChannelAccess<RmtTx, 0>, LED_BUFFER_SIZE>,
    pub leds: [RGB8; LED_COUNT],
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
// If you are using the ESP32-C6 - you shouldn't need to modify any of these!
//
// This alias describes the type of driver we will need. In this case, we
// are using the embassy-io driver with the ESP32-C6 USB_SERIAL_JTAG peripheral

pub type Rx = UsbSerialJtagRx<'static, Async>;
pub type Tx = UsbSerialJtagTx<'static, Async>;
pub type Storage = WireStorage<Rx, Tx, CriticalSectionRawMutex, 1024, 1024>;
pub type AppTx = EioWireTx<CriticalSectionRawMutex, Tx>;

/// AppRx is the type of our receiver, which is how we receive information from the client
pub type AppRx = WireRxImpl<UsbSerialJtagRx<'static, Async>>;
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
        | PingEndpoint              | blocking  | ping_handler                  |
        | GetUniqueIdEndpoint       | blocking  | unique_id_handler             |
        | SetSingleLedEndpoint      | blocking  | set_led_handler               |
        | SetAllLedEndpoint         | blocking  | set_all_led_handler           |
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
