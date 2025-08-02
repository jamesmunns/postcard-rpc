#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_hal::Async;
use esp_hal::clock::CpuClock;
use esp_hal::rmt::{Channel as RmtChannel, Rmt};
use esp_hal::time::Rate;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::usb_serial_jtag::{UsbSerialJtag, UsbSerialJtagRx, UsbSerialJtagTx};
use panic_rtt_target as _;

use defmt::info;

use esp_hal_smartled::{SmartLedsAdapter, smartLedBuffer};
use postcard_rpc::server::impls::embedded_io_async_v0_6::WireStorage;
use smart_leds::RGB8;
use smart_leds::{SmartLedsWrite, brightness, gamma};
use static_cell::ConstStaticCell;

use postcard_rpc::{
    define_dispatch,
    header::VarHeader,
    server::{
        Dispatch, Server,
        impls::embedded_io_async_v0_6::dispatch_impl::{
            WireRxBuf, WireRxImpl, WireSpawnImpl, WireTxImpl,
        },
    },
};

use workbook_icd::{
    BadPositionError, ENDPOINT_LIST, GetUniqueIdEndpoint, PingEndpoint, Rgb8, SetAllLedEndpoint,
    SetSingleLedEndpoint, SingleLed, TOPICS_IN_LIST, TOPICS_OUT_LIST,
};

struct Context {
    id: u64,
    led: SmartLedsAdapter<RmtChannel<esp_hal::Blocking, 0>, LED_BUFFER_SIZE>,
    leds: [RGB8; LED_COUNT],
}

impl Context {
    fn update_led(&mut self, position: usize, rgb: Rgb8) -> Result<(), BadPositionError> {
        if position >= LED_COUNT {
            return Err(BadPositionError);
        }
        let data = RGB8 {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        };
        self.leds[position] = brightness(gamma([data].into_iter()), 100).next().unwrap();
        Ok(())
    }

    fn write_leds(&mut self) -> Result<(), BadPositionError> {
        self.led.write(self.leds.iter().copied()).unwrap();
        Ok(())
    }
}

type Rx = UsbSerialJtagRx<'static, Async>;
type Tx = UsbSerialJtagTx<'static, Async>;
type Storage = WireStorage<Rx, Tx, CriticalSectionRawMutex, 1024, 1024>;
type AppTx = WireTxImpl<CriticalSectionRawMutex, UsbSerialJtagTx<'static, Async>>;
type AppRx = WireRxImpl<UsbSerialJtagRx<'static, Async>>;
type AppServer = Server<AppTx, AppRx, WireRxBuf, MyApp>;

static STORAGE: Storage = Storage::new();

define_dispatch! {
    app: MyApp;
    spawn_fn: spawn_fn;
    tx_impl: AppTx;
    spawn_impl: WireSpawnImpl;
    context: Context;

    endpoints: {
        list: ENDPOINT_LIST;

        | EndpointTy                | kind      | handler                       |
        | ----------                | ----      | -------                       |
        | PingEndpoint              | blocking  | ping_handler                  |
        | GetUniqueIdEndpoint       | blocking  | unique_id_handler             |
        | SetSingleLedEndpoint      | blocking  | set_led_handler               |
        | SetAllLedEndpoint         | blocking  | set_all_led_handler           |
    };
    topics_in: {
        list: TOPICS_IN_LIST;

        | TopicTy                   | kind      | handler                       |
        | ----------                | ----      | -------                       |
    };
    topics_out: {
        list: TOPICS_OUT_LIST;
    };
}

const LED_COUNT: usize = 1;
const LED_BUFFER_SIZE: usize = const { smartLedBuffer!(1).len() };

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    rtt_target::rtt_init_defmt!();

    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let timer0 = SystemTimer::new(p.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    let (erx, etx) = UsbSerialJtag::new(p.USB_DEVICE).into_async().split();
    let (rx_impl, tx_impl) = STORAGE.init(erx, etx).unwrap();

    let rmt = Rmt::new(p.RMT, Rate::from_mhz(80)).unwrap();

    let context = Context {
        id: 0,
        led: SmartLedsAdapter::new(rmt.channel0, p.GPIO8, <_>::default()),
        leds: <_>::default(),
    };

    static PACKET_RX_BUF: ConstStaticCell<[u8; 1024]> = ConstStaticCell::new([0u8; 1024]);

    let dispatcher = MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let mut server: AppServer =
        Server::new(tx_impl, rx_impl, PACKET_RX_BUF.take(), dispatcher, vkk);

    loop {
        let _ = server.run().await;
    }
}

// ---

fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping");
    rqst
}

fn unique_id_handler(context: &mut Context, _header: VarHeader, _rqst: ()) -> u64 {
    info!("unique_id");
    context.id
}

fn set_led_handler(
    context: &mut Context,
    _header: VarHeader,
    rqst: SingleLed,
) -> Result<(), BadPositionError> {
    info!("set_led");
    context.update_led(rqst.position as usize, rqst.rgb)?;
    context.write_leds()?;
    Ok(())
}

fn set_all_led_handler(context: &mut Context, _header: VarHeader, rqst: [Rgb8; 24]) {
    info!("set_all_led");
    for i in 0..LED_COUNT {
        context.update_led(i, rqst[i]).unwrap();
    }
    context.write_leds().unwrap();
}
