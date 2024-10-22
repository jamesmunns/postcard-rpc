use embassy_executor::{SpawnError, SpawnToken, Spawner};
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_usb_driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use futures_util::FutureExt;
use serde::Serialize;

use crate::{
    header::VarHeader,
    server2::{WireRx, WireRxErrorKind, WireSpawn, WireTx, WireTxErrorKind},
};

// pub fn eusb_wire_tx<D: Driver<'static>>(ep_in: D::EndpointIn, tx_buf: &'static mut [u8]) ->
pub mod dispatch_impl {
    pub const DEVICE_INTERFACE_GUIDS: &[&str] = &["{AFB9A6FB-30BA-44BC-9232-806CFC875321}"];

    use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
    use embassy_usb::{
        msos::{self, windows_version},
        Builder, Config, UsbDevice,
    };
    use embassy_usb_driver::Driver;
    use static_cell::{ConstStaticCell, StaticCell};

    pub type WireTxImpl<M, D> = super::EUsbWireTx<M, D>;
    pub type WireRxImpl<D> = super::EUsbWireRx<D>;
    pub type WireSpawnImpl = super::EUsbWireSpawn;
    pub type WireRxBuf = &'static mut [u8];

    pub use super::embassy_spawn as spawn_fn;
    use super::{EUsbWireRx, EUsbWireTx, EUsbWireTxInner, UsbDeviceBuffers};

    pub struct WireStorage<
        M: RawMutex + 'static,
        D: Driver<'static> + 'static,
        const CONFIG: usize = 256,
        const BOS: usize = 256,
        const CONTROL: usize = 64,
        const MSOS: usize = 256,
    > {
        bufs_usb: ConstStaticCell<UsbDeviceBuffers<CONFIG, BOS, CONTROL, CONFIG>>,
        cell: StaticCell<Mutex<M, EUsbWireTxInner<D>>>,
    }

    impl<
            M: RawMutex + 'static,
            D: Driver<'static> + 'static,
            const CONFIG: usize,
            const BOS: usize,
            const CONTROL: usize,
            const MSOS: usize,
        > WireStorage<M, D, CONFIG, BOS, CONTROL, MSOS>
    {
        pub const fn new() -> Self {
            Self {
                bufs_usb: ConstStaticCell::new(UsbDeviceBuffers::new()),
                cell: StaticCell::new(),
            }
        }

        pub fn init(
            &'static self,
            driver: D,
            config: Config<'static>,
            tx_buf: &'static mut [u8],
        ) -> (UsbDevice<'static, D>, WireTxImpl<M, D>, WireRxImpl<D>) {
            let bufs = self.bufs_usb.take();

            let mut builder = Builder::new(
                driver,
                config,
                &mut bufs.config_descriptor,
                &mut bufs.bos_descriptor,
                &mut bufs.msos_descriptor,
                &mut bufs.control_buf,
            );

            // Add the Microsoft OS Descriptor (MSOS/MOD) descriptor.
            // We tell Windows that this entire device is compatible with the "WINUSB" feature,
            // which causes it to use the built-in WinUSB driver automatically, which in turn
            // can be used by libusb/rusb software without needing a custom driver or INF file.
            // In principle you might want to call msos_feature() just on a specific function,
            // if your device also has other functions that still use standard class drivers.
            builder.msos_descriptor(windows_version::WIN8_1, 0);
            builder.msos_feature(msos::CompatibleIdFeatureDescriptor::new("WINUSB", ""));
            builder.msos_feature(msos::RegistryPropertyFeatureDescriptor::new(
                "DeviceInterfaceGUIDs",
                msos::PropertyData::RegMultiSz(DEVICE_INTERFACE_GUIDS),
            ));

            // Add a vendor-specific function (class 0xFF), and corresponding interface,
            // that uses our custom handler.
            let mut function = builder.function(0xFF, 0, 0);
            let mut interface = function.interface();
            let mut alt = interface.alt_setting(0xFF, 0, 0, None);
            let ep_out = alt.endpoint_bulk_out(64);
            let ep_in = alt.endpoint_bulk_in(64);
            drop(function);

            let wtx = self.cell.init(Mutex::new(EUsbWireTxInner {
                ep_in,
                _log_seq: 0,
                tx_buf,
                _max_log_len: 0,
            }));

            // Build the builder.
            let usb = builder.build();

            (usb, EUsbWireTx { inner: wtx }, EUsbWireRx { ep_out })
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// TX
//////////////////////////////////////////////////////////////////////////////

/// Implementation detail, holding the endpoint and scratch buffer used for sending
pub struct EUsbWireTxInner<D: Driver<'static>> {
    ep_in: D::EndpointIn,
    _log_seq: u32,
    tx_buf: &'static mut [u8],
    _max_log_len: usize,
}

#[derive(Copy)]
pub struct EUsbWireTx<M: RawMutex + 'static, D: Driver<'static> + 'static> {
    inner: &'static Mutex<M, EUsbWireTxInner<D>>,
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> Clone for EUsbWireTx<M, D> {
    fn clone(&self) -> Self {
        EUsbWireTx { inner: self.inner }
    }
}

impl<M: RawMutex + 'static, D: Driver<'static> + 'static> WireTx for EUsbWireTx<M, D> {
    type Error = WireTxErrorKind;

    async fn send<T: Serialize + ?Sized>(
        &self,
        hdr: VarHeader,
        msg: &T,
    ) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;

        let EUsbWireTxInner {
            ep_in,
            _log_seq: _,
            tx_buf,
            _max_log_len: _,
        }: &mut EUsbWireTxInner<D> = &mut inner;

        let (hdr_used, remain) = hdr.write_to_slice(tx_buf).ok_or(WireTxErrorKind::Other)?;
        let bdy_used = postcard::to_slice(msg, remain).map_err(|_| WireTxErrorKind::Other)?;
        let used_ttl = hdr_used.len() + bdy_used.len();

        if let Some(used) = tx_buf.get(..used_ttl) {
            send_all::<D>(ep_in, used).await
        } else {
            Err(WireTxErrorKind::Other)
        }
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut inner = self.inner.lock().await;
        send_all::<D>(&mut inner.ep_in, buf).await
    }
}

#[inline]
async fn send_all<D>(ep_in: &mut D::EndpointIn, out: &[u8]) -> Result<(), WireTxErrorKind>
where
    D: Driver<'static>,
{
    if out.is_empty() {
        return Ok(());
    }
    // TODO: Timeout?
    if ep_in.wait_enabled().now_or_never().is_none() {
        return Ok(());
    }

    // write in segments of 64. The last chunk may
    // be 0 < len <= 64.
    for ch in out.chunks(64) {
        if ep_in.write(ch).await.is_err() {
            return Err(WireTxErrorKind::ConnectionClosed);
        }
    }
    // If the total we sent was a multiple of 64, send an
    // empty message to "flush" the transaction. We already checked
    // above that the len != 0.
    if (out.len() & (64 - 1)) == 0 && ep_in.write(&[]).await.is_err() {
        return Err(WireTxErrorKind::ConnectionClosed);
    }

    Ok(())
}

//////////////////////////////////////////////////////////////////////////////
// RX
//////////////////////////////////////////////////////////////////////////////

pub struct EUsbWireRx<D: Driver<'static>> {
    ep_out: D::EndpointOut,
}

impl<D: Driver<'static>> WireRx for EUsbWireRx<D> {
    type Error = WireRxErrorKind;

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        let buflen = buf.len();
        let mut window = &mut buf[..];
        while !window.is_empty() {
            let n = match self.ep_out.read(window).await {
                Ok(n) => n,
                Err(EndpointError::BufferOverflow) => {
                    return Err(WireRxErrorKind::ReceivedMessageTooLarge)
                }
                Err(EndpointError::Disabled) => return Err(WireRxErrorKind::ConnectionClosed),
            };

            let (_now, later) = window.split_at_mut(n);
            window = later;
            if n != 64 {
                // We now have a full frame! Great!
                let wlen = window.len();
                let len = buflen - wlen;
                let frame = &mut buf[..len];

                return Ok(frame);
            }
        }

        // If we got here, we've run out of space. That's disappointing. Accumulate to the
        // end of this packet
        loop {
            match self.ep_out.read(buf).await {
                Ok(64) => {}
                Ok(_) => return Err(WireRxErrorKind::ReceivedMessageTooLarge),
                Err(EndpointError::BufferOverflow) => {
                    return Err(WireRxErrorKind::ReceivedMessageTooLarge)
                }
                Err(EndpointError::Disabled) => return Err(WireRxErrorKind::ConnectionClosed),
            };
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// SPAWN
//////////////////////////////////////////////////////////////////////////////

// todo: just use a standard tokio impl?
#[derive(Clone)]
pub struct EUsbWireSpawn {
    pub spawner: Spawner,
}

impl From<Spawner> for EUsbWireSpawn {
    fn from(value: Spawner) -> Self {
        Self { spawner: value }
    }
}

impl WireSpawn for EUsbWireSpawn {
    type Error = SpawnError;

    type Info = Spawner;

    fn info(&self) -> &Self::Info {
        &self.spawner
    }
}

pub fn embassy_spawn<Sp, S: Sized>(sp: &Sp, tok: SpawnToken<S>) -> Result<(), Sp::Error>
where
    Sp: WireSpawn<Error = SpawnError, Info = Spawner>,
{
    let info = sp.info();
    info.spawn(tok)
}

//////////////////////////////////////////////////////////////////////////////
// OTHER
//////////////////////////////////////////////////////////////////////////////

pub struct UsbDeviceBuffers<
    const CONFIG: usize = 256,
    const BOS: usize = 256,
    const CONTROL: usize = 64,
    const MSOS: usize = 256,
> {
    pub config_descriptor: [u8; CONFIG],
    pub bos_descriptor: [u8; BOS],
    pub control_buf: [u8; CONTROL],
    pub msos_descriptor: [u8; MSOS],
}

impl<const CONFIG: usize, const BOS: usize, const CONTROL: usize, const MSOS: usize>
    UsbDeviceBuffers<CONFIG, BOS, CONTROL, MSOS>
{
    pub const fn new() -> Self {
        Self {
            config_descriptor: [0u8; CONFIG],
            bos_descriptor: [0u8; BOS],
            msos_descriptor: [0u8; MSOS],
            control_buf: [0u8; CONTROL],
        }
    }
}

pub struct PacketBuffers<const TX: usize = 1024, const RX: usize = 1024> {
    pub tx_buf: [u8; TX],
    pub rx_buf: [u8; RX],
}

impl<const TX: usize, const RX: usize> PacketBuffers<TX, RX> {
    pub const fn new() -> Self {
        Self {
            tx_buf: [0u8; TX],
            rx_buf: [0u8; RX],
        }
    }
}

/// This is a basic example that everything compiles. It is intended to exercise the macro above,
/// as well as provide impls for docs. Don't rely on any of this!
#[doc(hidden)]
#[allow(dead_code)]
#[cfg(feature = "test-utils")]
pub mod fake {
    use crate::{
        define_dispatch2, endpoints,
        server2::{Sender, SpawnContext},
        topics,
    };
    use crate::{header::VarHeader, Schema};
    use embassy_usb_driver::{Bus, ControlPipe, EndpointIn, EndpointOut};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Schema)]
    pub struct AReq(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct AResp(pub u8);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BReq(pub u16);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct BResp(pub u32);
    #[derive(Serialize, Deserialize, Schema)]
    pub struct GReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct GResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct DReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct DResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct EReq;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct EResp;
    #[derive(Serialize, Deserialize, Schema)]
    pub struct ZMsg(pub i16);

    endpoints! {
        list = ENDPOINT_LIST;
        | EndpointTy        | RequestTy     | ResponseTy    | Path              |
        | ----------        | ---------     | ----------    | ----              |
        | AlphaEndpoint     | AReq          | AResp         | "alpha"           |
        | BetaEndpoint      | BReq          | BResp         | "beta"            |
        | GammaEndpoint     | GReq          | GResp         | "gamma"           |
        | DeltaEndpoint     | DReq          | DResp         | "delta"           |
        | EpsilonEndpoint   | EReq          | EResp         | "epsilon"         |
    }

    topics! {
        list = TOPICS_IN_LIST;
        | TopicTy           | MessageTy     | Path              |
        | ----------        | ---------     | ----              |
        | ZetaTopic1        | ZMsg          | "zeta1"           |
        | ZetaTopic2        | ZMsg          | "zeta2"           |
        | ZetaTopic3        | ZMsg          | "zeta3"           |
    }

    pub struct FakeMutex;
    pub struct FakeDriver;
    pub struct FakeEpOut;
    pub struct FakeEpIn;
    pub struct FakeCtlPipe;
    pub struct FakeBus;

    impl embassy_usb_driver::Endpoint for FakeEpOut {
        fn info(&self) -> &embassy_usb_driver::EndpointInfo {
            todo!()
        }

        async fn wait_enabled(&mut self) {
            todo!()
        }
    }

    impl EndpointOut for FakeEpOut {
        async fn read(
            &mut self,
            _buf: &mut [u8],
        ) -> Result<usize, embassy_usb_driver::EndpointError> {
            todo!()
        }
    }

    impl embassy_usb_driver::Endpoint for FakeEpIn {
        fn info(&self) -> &embassy_usb_driver::EndpointInfo {
            todo!()
        }

        async fn wait_enabled(&mut self) {
            todo!()
        }
    }

    impl EndpointIn for FakeEpIn {
        async fn write(&mut self, _buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
            todo!()
        }
    }

    impl ControlPipe for FakeCtlPipe {
        fn max_packet_size(&self) -> usize {
            todo!()
        }

        async fn setup(&mut self) -> [u8; 8] {
            todo!()
        }

        async fn data_out(
            &mut self,
            _buf: &mut [u8],
            _first: bool,
            _last: bool,
        ) -> Result<usize, embassy_usb_driver::EndpointError> {
            todo!()
        }

        async fn data_in(
            &mut self,
            _data: &[u8],
            _first: bool,
            _last: bool,
        ) -> Result<(), embassy_usb_driver::EndpointError> {
            todo!()
        }

        async fn accept(&mut self) {
            todo!()
        }

        async fn reject(&mut self) {
            todo!()
        }

        async fn accept_set_address(&mut self, _addr: u8) {
            todo!()
        }
    }

    impl Bus for FakeBus {
        async fn enable(&mut self) {
            todo!()
        }

        async fn disable(&mut self) {
            todo!()
        }

        async fn poll(&mut self) -> embassy_usb_driver::Event {
            todo!()
        }

        fn endpoint_set_enabled(
            &mut self,
            _ep_addr: embassy_usb_driver::EndpointAddress,
            _enabled: bool,
        ) {
            todo!()
        }

        fn endpoint_set_stalled(
            &mut self,
            _ep_addr: embassy_usb_driver::EndpointAddress,
            _stalled: bool,
        ) {
            todo!()
        }

        fn endpoint_is_stalled(&mut self, _ep_addr: embassy_usb_driver::EndpointAddress) -> bool {
            todo!()
        }

        async fn remote_wakeup(&mut self) -> Result<(), embassy_usb_driver::Unsupported> {
            todo!()
        }
    }

    impl embassy_usb_driver::Driver<'static> for FakeDriver {
        type EndpointOut = FakeEpOut;

        type EndpointIn = FakeEpIn;

        type ControlPipe = FakeCtlPipe;

        type Bus = FakeBus;

        fn alloc_endpoint_out(
            &mut self,
            _ep_type: embassy_usb_driver::EndpointType,
            _max_packet_size: u16,
            _interval_ms: u8,
        ) -> Result<Self::EndpointOut, embassy_usb_driver::EndpointAllocError> {
            todo!()
        }

        fn alloc_endpoint_in(
            &mut self,
            _ep_type: embassy_usb_driver::EndpointType,
            _max_packet_size: u16,
            _interval_ms: u8,
        ) -> Result<Self::EndpointIn, embassy_usb_driver::EndpointAllocError> {
            todo!()
        }

        fn start(self, _control_max_packet_size: u16) -> (Self::Bus, Self::ControlPipe) {
            todo!()
        }
    }

    unsafe impl embassy_sync::blocking_mutex::raw::RawMutex for FakeMutex {
        const INIT: Self = Self;

        fn lock<R>(&self, _f: impl FnOnce() -> R) -> R {
            todo!()
        }
    }

    pub struct TestContext {
        pub a: u32,
        pub b: u32,
    }

    impl SpawnContext for TestContext {
        type SpawnCtxt = TestSpawnContext;

        fn spawn_ctxt(&mut self) -> Self::SpawnCtxt {
            TestSpawnContext { b: self.b }
        }
    }

    pub struct TestSpawnContext {
        b: u32,
    }

    // TODO: How to do module path concat?
    use crate::server2::impls::embassy_usb_v0_3::dispatch_impl::{
        spawn_fn, WireSpawnImpl, WireTxImpl,
    };

    define_dispatch2! {
        app: SingleDispatcher;
        spawn_fn: spawn_fn;
        tx_impl: WireTxImpl<FakeMutex, FakeDriver>;
        spawn_impl: WireSpawnImpl;
        context: TestContext;

        endpoints: {
            list: ENDPOINT_LIST;

            | EndpointTy        | kind      | handler                   |
            | ----------        | ----      | -------                   |
            | AlphaEndpoint     | async     | test_alpha_handler        |
            | EpsilonEndpoint   | spawn     | test_epsilon_handler_task |
        };
        topics_in: {
            list: TOPICS_IN_LIST;

            | TopicTy           | kind      | handler               |
            | ----------        | ----      | -------               |
            // | ZetaTopic1        | blocking  | test_zeta_blocking    |
            // | ZetaTopic2        | async     | test_zeta_async       |
            // | ZetaTopic3        | spawn     | test_zeta_spawn       |
        };
    }

    async fn test_alpha_handler(
        _context: &mut TestContext,
        _header: VarHeader,
        _body: AReq,
    ) -> AResp {
        todo!()
    }

    async fn test_beta_handler(
        _context: &mut TestContext,
        _header: VarHeader,
        _body: BReq,
    ) -> BResp {
        todo!()
    }

    async fn test_gamma_handler(
        _context: &mut TestContext,
        _header: VarHeader,
        _body: GReq,
    ) -> GResp {
        todo!()
    }

    fn test_delta_handler(_context: &mut TestContext, _header: VarHeader, _body: DReq) -> DResp {
        todo!()
    }

    #[embassy_executor::task]
    async fn test_epsilon_handler_task(
        _context: TestSpawnContext,
        _header: VarHeader,
        _body: EReq,
        _sender: Sender<WireTxImpl<FakeMutex, FakeDriver>>,
    ) {
        todo!()
    }
}
