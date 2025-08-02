#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::{PIO0, SPI0, USB},
    pio::Pio,
    spi::{self, Spi},
    usb,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::{Delay, Duration, Ticker};
use embassy_usb::{Config, UsbDevice};
use embedded_hal_bus::spi::ExclusiveDevice;
use lis3dh_async::{Lis3dh, Lis3dhSPI};
use portable_atomic::{AtomicBool, Ordering};
use postcard_rpc::{
    define_dispatch,
    header::VarHeader,
    server::{
        impls::embassy_usb_v0_5::{
            dispatch_impl::{
                spawn_fn, WireRxBuf, WireRxImpl, WireSpawnImpl, WireStorage, WireTxImpl,
            },
            PacketBuffers,
        },
        Dispatch, Sender, Server, SpawnContext,
    },
};
use smart_leds::{colors::BLACK, RGB8};
use static_cell::{ConstStaticCell, StaticCell};
use workbook_fw::{
    get_unique_id,
    ws2812::{self, Ws2812},
    Irqs,
};
use workbook_icd::{
    AccelTopic, Acceleration, BadPositionError, GetUniqueIdEndpoint, PingEndpoint, Rgb8,
    SetAllLedEndpoint, SetSingleLedEndpoint, SingleLed, StartAccel, StartAccelerationEndpoint,
    StopAccelerationEndpoint, ENDPOINT_LIST, TOPICS_IN_LIST, TOPICS_OUT_LIST,
};
use {defmt_rtt as _, panic_probe as _};

pub type Accel =
    Lis3dh<Lis3dhSPI<ExclusiveDevice<Spi<'static, SPI0, spi::Async>, Output<'static>, Delay>>>;
static ACCEL: StaticCell<Mutex<ThreadModeRawMutex, Accel>> = StaticCell::new();

pub struct Context {
    pub unique_id: u64,
    pub ws2812: Ws2812<'static, PIO0, 0, 24>,
    pub ws2812_state: [RGB8; 24],
    pub accel: &'static Mutex<ThreadModeRawMutex, Accel>,
}

pub struct SpawnCtx {
    pub accel: &'static Mutex<ThreadModeRawMutex, Accel>,
}

impl SpawnContext for Context {
    type SpawnCtxt = SpawnCtx;
    fn spawn_ctxt(&mut self) -> Self::SpawnCtxt {
        SpawnCtx { accel: self.accel }
    }
}

type AppDriver = usb::Driver<'static, USB>;
type AppStorage = WireStorage<ThreadModeRawMutex, AppDriver, 256, 256, 64, 256>;
type BufStorage = PacketBuffers<1024, 1024>;
type AppTx = WireTxImpl<ThreadModeRawMutex, AppDriver>;
type AppRx = WireRxImpl<AppDriver>;
type AppServer = Server<AppTx, AppRx, WireRxBuf, MyApp>;

static PBUFS: ConstStaticCell<BufStorage> = ConstStaticCell::new(BufStorage::new());
static STORAGE: AppStorage = AppStorage::new();

fn usb_config() -> Config<'static> {
    let mut config = Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("OneVariable");
    config.product = Some("ov-twin");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config
}

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
        | SetSingleLedEndpoint      | async     | set_led_handler               |
        | SetAllLedEndpoint         | async     | set_all_led_handler           |
        | StartAccelerationEndpoint | spawn     | accelerometer_handler         |
        | StopAccelerationEndpoint  | blocking  | accelerometer_stop_handler    |
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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // SYSTEM INIT
    info!("Start");
    let mut p = embassy_rp::init(Default::default());
    let unique_id = defmt::unwrap!(get_unique_id(p.FLASH.reborrow()));
    info!("id: {=u64:016X}", unique_id);

    // PIO/WS2812 INIT
    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, ws2812::Irqs);
    let ws2812: Ws2812<'static, PIO0, 0, 24> = Ws2812::new(&mut common, sm0, p.DMA_CH0, p.PIN_25);

    // SPI INIT
    let spi = Spi::new(
        p.SPI0,
        p.PIN_6, // clk
        p.PIN_7, // mosi
        p.PIN_4, // miso
        p.DMA_CH1,
        p.DMA_CH2,
        spi::Config::default(),
    );
    // CS: GPIO5
    let bus = ExclusiveDevice::new(spi, Output::new(p.PIN_5, Level::High), Delay);
    let acc: Accel = defmt::unwrap!(Lis3dh::new_spi(bus).await.map_err(drop));
    let accel_ref = ACCEL.init(Mutex::new(acc));

    // USB/RPC INIT
    let driver = usb::Driver::new(p.USB, Irqs);
    let pbufs = PBUFS.take();
    let config = usb_config();

    let context = Context {
        unique_id,
        ws2812,
        ws2812_state: [BLACK; 24],
        accel: accel_ref,
    };

    let (device, tx_impl, rx_impl) = STORAGE.init(driver, config, pbufs.tx_buf.as_mut_slice());

    // Set timeout to 4ms/frame, instead of the default 2ms/frame
    tx_impl.set_timeout_ms_per_frame(4).await;

    let dispatcher = MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let mut server: AppServer = Server::new(
        tx_impl,
        rx_impl,
        pbufs.rx_buf.as_mut_slice(),
        dispatcher,
        vkk,
    );
    spawner.must_spawn(usb_task(device));

    loop {
        // If the host disconnects, we'll return an error here.
        // If this happens, just wait until the host reconnects
        let _ = server.run().await;
    }
}

/// This handles the low level USB management
#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, AppDriver>) {
    usb.run().await;
}

// ---

fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping");
    rqst
}

fn unique_id_handler(context: &mut Context, _header: VarHeader, _rqst: ()) -> u64 {
    info!("unique_id");
    context.unique_id
}

async fn set_led_handler(
    context: &mut Context,
    _header: VarHeader,
    rqst: SingleLed,
) -> Result<(), BadPositionError> {
    info!("set_led");
    if rqst.position >= 24 {
        return Err(BadPositionError);
    }
    let pos = rqst.position as usize;
    context.ws2812_state[pos] = RGB8 {
        r: rqst.rgb.r,
        g: rqst.rgb.g,
        b: rqst.rgb.b,
    };
    context.ws2812.write(&context.ws2812_state).await;
    Ok(())
}

async fn set_all_led_handler(context: &mut Context, _header: VarHeader, rqst: [Rgb8; 24]) {
    info!("set_all_led");
    context
        .ws2812_state
        .iter_mut()
        .zip(rqst.iter())
        .for_each(|(s, rgb)| {
            s.r = rgb.r;
            s.g = rgb.g;
            s.b = rgb.b;
        });
    context.ws2812.write(&context.ws2812_state).await;
}

static STOP: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task]
async fn accelerometer_handler(
    context: SpawnCtx,
    header: VarHeader,
    rqst: StartAccel,
    sender: Sender<AppTx>,
) {
    let mut accel = context.accel.lock().await;
    if sender
        .reply::<StartAccelerationEndpoint>(header.seq_no, &())
        .await
        .is_err()
    {
        defmt::error!("Failed to reply, stopping accel");
        return;
    }

    defmt::unwrap!(accel.set_range(lis3dh_async::Range::G8).await.map_err(drop));

    let mut ticker = Ticker::every(Duration::from_millis(rqst.interval_ms.into()));
    let mut seq = 0u8;
    while !STOP.load(Ordering::Acquire) {
        ticker.next().await;
        let acc = defmt::unwrap!(accel.accel_raw().await.map_err(drop));
        defmt::println!("ACC: {=i16},{=i16},{=i16}", acc.x, acc.y, acc.z);
        let msg = Acceleration {
            x: acc.x,
            y: acc.y,
            z: acc.z,
        };
        if sender
            .publish::<AccelTopic>(seq.into(), &msg)
            .await
            .is_err()
        {
            defmt::error!("Send error!");
            break;
        }
        seq = seq.wrapping_add(1);
    }
    defmt::info!("Stopping!");
    STOP.store(false, Ordering::Release);
}

fn accelerometer_stop_handler(context: &mut Context, _header: VarHeader, _rqst: ()) -> bool {
    info!("accel_stop");
    let was_busy = context.accel.try_lock().is_err();
    if was_busy {
        STOP.store(true, Ordering::Release);
    }
    was_busy
}
