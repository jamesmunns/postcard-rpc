//! Implementation of transport using nusb

use std::future::Future;

use nusb::{
    descriptors::TransferType,
    io::{EndpointRead, EndpointWrite},
    transfer::{Bulk, Direction, In, Out, TransferError},
    Interface,
};
use postcard_schema::Schema;
use serde::de::DeserializeOwned;

use crate::{
    header::VarSeqKind,
    host_client::{HostClient, WireRx, WireSpawn, WireTx},
};

// TODO: These should all be configurable, PRs welcome

/// The size in bytes of the largest possible IN transfer
pub(crate) const MAX_TRANSFER_SIZE: usize = 1024;
/// How many in-flight requests at once - allows nusb to keep pulling frames
/// even if we haven't processed them host-side yet.
pub(crate) const IN_FLIGHT_REQS: usize = 4;

/// # `nusb` Constructor Methods
///
/// These methods are used to create a new [HostClient] instance for use with `nusb` and
/// USB bulk transfer encoding.
///
/// **Requires feature**: `raw-nusb`
impl<WireErr> HostClient<WireErr>
where
    WireErr: DeserializeOwned + Schema,
{
    /// Try to create a new link using [`nusb`] for connectivity
    ///
    /// The provided function will be used to find a matching device. The first
    /// matching device will be connected to. `err_uri_path` is
    /// the path associated with the `WireErr` message type.
    ///
    /// Returns an error if no device could be found, or if there was an error
    /// connecting to the device.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Platform specific support
    ///
    /// When using Windows, the WinUSB driver does not allow enumerating interfaces.
    /// When on windows, this method will ALWAYS try to connect to interface zero.
    /// This limitation may be removed in the future, and if so, will be changed to
    /// look for the first interface with the class of 0xFF.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let client = HostClient::<Error>::try_new_raw_nusb(
    ///         // Find the first device with the serial 12345678
    ///         |d| d.serial_number() == Some("12345678"),
    ///         // the URI/path for `Error` messages
    ///         "error",
    ///         // Outgoing queue depth in messages
    ///         8,
    ///         // Use one-byte sequence numbers
    ///         VarSeqKind::Seq1,
    ///     )
    ///     .await
    ///     .unwrap();
    /// }
    /// ```
    pub async fn try_new_raw_nusb<F: FnMut(&nusb::DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let x = nusb::list_devices()
            .await
            .map_err(|e| format!("Error listing devices: {e:?}"))?
            .find(func)
            .ok_or_else(|| String::from("Failed to find matching nusb device!"))?;

        // NOTE: We can't enumerate interfaces on Windows. For now, just use
        // a hardcoded interface of zero instead of trying to find the right one
        #[cfg(not(target_os = "windows"))]
        let interface_id = x
            .interfaces()
            .position(|i| i.class() == 0xFF)
            .ok_or_else(|| String::from("Failed to find matching interface!!"))?;

        #[cfg(target_os = "windows")]
        let interface_id = 0;

        Self::try_from_nusb_and_interface_id(
            &x,
            interface_id,
            err_uri_path,
            outgoing_depth,
            seq_no_kind,
        )
        .await
    }

    /// Try to create a new link using [`nusb`] for connectivity
    ///
    /// The provided function will be used to find a matching device and interface. The first
    /// matching device will be connected to. `err_uri_path` is
    /// the path associated with the `WireErr` message type.
    ///
    /// Returns an error if no device or interface could be found, or if there was an error
    /// connecting to the device or interface.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Platform specific support
    ///
    /// When using Windows, the WinUSB driver does not allow enumerating interfaces.
    /// Therefore, this constructor is not available on windows. This limitation may
    /// be removed in the future.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let client = HostClient::<Error>::try_new_raw_nusb_with_interface(
    ///         // Find the first device with the serial 12345678
    ///         |d| d.serial_number() == Some("12345678"),
    ///         // Find the "Vendor Specific" interface
    ///         |i| i.class() == 0xFF,
    ///         // the URI/path for `Error` messages
    ///         "error",
    ///         // Outgoing queue depth in messages
    ///         8,
    ///         // Use one-byte sequence numbers
    ///         VarSeqKind::Seq1,
    ///     )
    ///     .await
    ///     .unwrap();
    /// }
    /// ```
    #[cfg(not(target_os = "windows"))]
    pub async fn try_new_raw_nusb_with_interface<
        F1: FnMut(&nusb::DeviceInfo) -> bool,
        F2: FnMut(&nusb::InterfaceInfo) -> bool,
    >(
        device_func: F1,
        interface_func: F2,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let x = nusb::list_devices()
            .await
            .map_err(|e| format!("Error listing devices: {e:?}"))?
            .find(device_func)
            .ok_or_else(|| String::from("Failed to find matching nusb device!"))?;
        let interface_id = x
            .interfaces()
            .position(interface_func)
            .ok_or_else(|| String::from("Failed to find matching interface!!"))?;

        Self::try_from_nusb_and_interface_id(
            &x,
            interface_id,
            err_uri_path,
            outgoing_depth,
            seq_no_kind,
        )
        .await
    }

    /// Try to create a new link using [`nusb`] for connectivity
    ///
    /// This will connect to the given device and interface. `err_uri_path` is
    /// the path associated with the `WireErr` message type.
    ///
    /// Returns an error if there was an error connecting to the device or interface.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    ///
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     // Assume the first usb device is the one we're interested
    ///     let dev = nusb::list_devices().await.unwrap().next().unwrap();
    ///     let client = HostClient::<Error>::try_from_nusb_and_interface_id(
    ///         // Device to open
    ///         &dev,
    ///         // Use the first interface (0)
    ///         0,
    ///         // the URI/path for `Error` messages
    ///         "error",
    ///         // Outgoing queue depth in messages
    ///         8,
    ///         // Use one-byte sequence numbers
    ///         VarSeqKind::Seq1,
    ///     )
    ///     .await
    ///     .unwrap();
    /// }
    /// ```
    pub async fn try_from_nusb_and_interface_id(
        dev: &nusb::DeviceInfo,
        interface_id: usize,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let dev = dev
            .open()
            .await
            .map_err(|e| format!("Failed opening device: {e:?}"))?;
        let interface = dev
            .claim_interface(interface_id as u8)
            .await
            .map_err(|e| format!("Failed claiming interface: {e:?}"))?;

        Self::try_from_nusb_interface(interface, err_uri_path, outgoing_depth, seq_no_kind).await
    }

    /// Try to create a new link using [`nusb`] for connectivity from a claimed nusb interface
    ///
    /// `err_uri_path` is the path associated with the `WireErr` message type.
    ///
    /// Returns an error if there was an error instantiating the transport.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    ///
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     // Assume the first usb device is the one we're interested
    ///     let info = nusb::list_devices().await.unwrap().next().unwrap();
    ///     let dev = info.open().await.unwrap();
    ///
    ///     // Assume the first device interface is the one we're interested
    ///     let interface = dev.claim_interface(0).await.unwrap();
    ///
    ///     let client = HostClient::<Error>::try_from_nusb_interface(
    ///         // The claimed interface
    ///         interface,
    ///         // the URI/path for `Error` messages
    ///         "error",
    ///         // Outgoing queue depth in messages
    ///         8,
    ///         // Use one-byte sequence numbers
    ///         VarSeqKind::Seq1,
    ///     )
    ///     .await
    ///     .unwrap();
    /// }
    /// ```
    pub async fn try_from_nusb_interface(
        interface: Interface,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let mut mps: Option<usize> = None;
        let mut ep_in: Option<u8> = None;
        let mut ep_out: Option<u8> = None;
        for ias in interface.descriptors() {
            for ep in ias
                .endpoints()
                .filter(|e| e.transfer_type() == TransferType::Bulk)
            {
                match ep.direction() {
                    Direction::Out => {
                        mps = Some(match mps.take() {
                            Some(old) => old.min(ep.max_packet_size()),
                            None => ep.max_packet_size(),
                        });
                        ep_out = Some(ep.address());
                    }
                    Direction::In => ep_in = Some(ep.address()),
                }
            }
        }

        if let Some(max_packet_size) = &mps {
            tracing::debug!(max_packet_size, "Detected max packet size");
        } else {
            tracing::warn!("Unable to detect Max Packet Size!");
        };

        let ep_out = ep_out.ok_or("Failed to find OUT EP")?;
        tracing::debug!("OUT EP: {ep_out}");

        let ep_in = ep_in.ok_or("Failed to find IN EP")?;
        tracing::debug!("IN EP: {ep_in}");

        let writer = interface
            .endpoint::<Bulk, Out>(ep_out)
            .map_err(|e| format!("Failed to claim OUT endpoint: {e:?}"))?
            .writer(MAX_TRANSFER_SIZE)
            .with_num_transfers(IN_FLIGHT_REQS);

        let reader = interface
            .endpoint::<Bulk, In>(ep_in)
            .map_err(|e| format!("Failed to claim IN endpoint: {e:?}"))?
            .reader(MAX_TRANSFER_SIZE)
            .with_num_transfers(IN_FLIGHT_REQS);

        Ok(HostClient::new_with_wire(
            NusbWireTx { writer },
            NusbWireRx { reader },
            NusbSpawn,
            seq_no_kind,
            err_uri_path,
            outgoing_depth,
        ))
    }

    /// Create a new link using [`nusb`] for connectivity
    ///
    /// Panics if connection fails. See [`Self::try_new_raw_nusb()`] for more details.
    ///
    /// This constructor is available when the `raw-nusb` feature is enabled.
    ///
    /// ## Example
    ///
    /// ```rust,no_run
    /// use postcard_rpc::host_client::HostClient;
    /// use postcard_rpc::header::VarSeqKind;
    /// use serde::{Serialize, Deserialize};
    /// use postcard_schema::Schema;
    ///
    /// /// A "wire error" type your server can use to respond to any
    /// /// kind of request, for example if deserializing a request fails
    /// #[derive(Debug, PartialEq, Schema, Serialize, Deserialize)]
    /// pub enum Error {
    ///    SomethingBad
    /// }
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() {
    ///     let client = HostClient::<Error>::new_raw_nusb(
    ///         // Find the first device with the serial 12345678
    ///         |d| d.serial_number() == Some("12345678"),
    ///         // the URI/path for `Error` messages
    ///         "error",
    ///         // Outgoing queue depth in messages
    ///         8,
    ///         // Use one-byte sequence numbers
    ///         VarSeqKind::Seq1,
    ///     )
    ///     .await;
    /// }
    /// ```
    pub async fn new_raw_nusb<F: FnMut(&nusb::DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Self {
        Self::try_new_raw_nusb(func, err_uri_path, outgoing_depth, seq_no_kind)
            .await
            .expect("should have found nusb device")
    }
}

//////////////////////////////////////////////////////////////////////////////
// Wire Interface Implementation
//////////////////////////////////////////////////////////////////////////////

/// NUSB Wire Interface Implementor
///
/// Uses Tokio for spawning tasks on non-wasm targets
/// Uses spawn_local on wasm
struct NusbSpawn;

#[cfg(not(target_family = "wasm"))]
impl WireSpawn for NusbSpawn {
    fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) {
        // Explicitly drop the joinhandle as it impls Future and this makes
        // clippy mad if you just let it drop implicitly
        core::mem::drop(tokio::task::spawn(fut));
    }
}

#[cfg(target_family = "wasm")]
impl WireSpawn for NusbSpawn {
    fn spawn(&mut self, fut: impl Future<Output = ()> + 'static) {
        wasm_bindgen_futures::spawn_local(fut);
    }
}

/// NUSB 0.2 Wire Transmit Interface Implementor
struct NusbWireTx {
    pub writer: EndpointWrite<Bulk>,
}

#[derive(thiserror::Error, Debug)]
enum NusbWireTxError {
    #[error("Transfer Error on Send")]
    Transfer(#[from] TransferError),
    #[error("I/O Error on Send")]
    Io(#[from] std::io::Error),
}

impl WireTx for NusbWireTx {
    type Error = NusbWireTxError;

    #[inline]
    #[cfg(not(target_family = "wasm"))]
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.send_inner(data)
    }

    #[inline]
    #[cfg(target_family = "wasm")]
    fn send(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), Self::Error>> {
        self.send_inner(data)
    }
}

impl NusbWireTx {
    async fn send_inner(&mut self, data: Vec<u8>) -> Result<(), NusbWireTxError> {
        use tokio::io::AsyncWriteExt;

        self.writer.write_all(&data).await?;
        self.writer.flush_end_async().await?;

        Ok(())
    }
}

/// NUSB 0.2 Wire Receive Interface Implementor
struct NusbWireRx {
    pub reader: EndpointRead<Bulk>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum NusbWireRxError {
    #[error("Transfer Error on Recv")]
    Transfer(#[from] TransferError),
    #[error("I/O Error on Recv")]
    IO(#[from] std::io::Error),
    #[error("Short Packet Error From nusb")]
    ExpectedShortPacket(#[from] nusb::io::ExpectedShortPacket),
}

impl WireRx for NusbWireRx {
    type Error = NusbWireRxError;

    #[inline]
    #[cfg(not(target_family = "wasm"))]
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        self.recv_inner()
    }

    #[inline]
    #[cfg(target_family = "wasm")]
    fn receive(&mut self) -> impl Future<Output = Result<Vec<u8>, Self::Error>> {
        self.recv_inner()
    }
}

impl NusbWireRx {
    async fn recv_inner(&mut self) -> Result<Vec<u8>, NusbWireRxError> {
        use tokio::io::AsyncReadExt;

        let mut reader = self.reader.until_short_packet();
        let mut v = Vec::new();

        reader.read_to_end(&mut v).await?;
        reader.consume_end()?;

        Ok(v)
    }
}
