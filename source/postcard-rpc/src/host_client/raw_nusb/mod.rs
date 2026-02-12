//! Implementation of transport using nusb

#[cfg(feature = "raw-nusb-0_1")]
mod nusb_0_1;

#[cfg(feature = "raw-nusb-0_2")]
mod nusb_0_2;

// Default to use nusb 0.1
#[cfg(feature = "raw-nusb-0_1")]
use nusb_0_1 as _impl;

// otherwise use nusb 0.2 if raw-nusb-0_1 is not specified
#[cfg(all(not(feature = "raw-nusb-0_1"), feature = "raw-nusb-0_2"))]
use nusb_0_2 as _impl;

use _impl as nusb;

use std::future::Future;

use nusb::{transfer::Direction, DeviceInfo};
use postcard_schema::Schema;
use serde::de::DeserializeOwned;

use crate::{
    header::VarSeqKind,
    host_client::{HostClient, WireSpawn},
};

// TODO: These should all be configurable, PRs welcome

/// The size in bytes of the largest possible IN transfer
pub(crate) const MAX_TRANSFER_SIZE: usize = 1024;
/// How many in-flight requests at once - allows nusb to keep pulling frames
/// even if we haven't processed them host-side yet.
pub(crate) const IN_FLIGHT_REQS: usize = 4;
/// How many consecutive IN errors will we try to recover from before giving up?
pub(crate) const MAX_STALL_RETRIES: usize = 10;

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
    /// let client = HostClient::<Error>::try_new_raw_nusb(
    ///     // Find the first device with the serial 12345678
    ///     |d| d.serial_number() == Some("12345678"),
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Use one-byte sequence numbers
    ///     VarSeqKind::Seq1,
    /// ).unwrap();
    /// ```
    pub fn try_new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let x = nusb::list_devices()
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

        Self::try_from_nusb_and_interface(
            &x,
            interface_id,
            err_uri_path,
            outgoing_depth,
            seq_no_kind,
        )
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
    /// let client = HostClient::<Error>::try_new_raw_nusb_with_interface(
    ///     // Find the first device with the serial 12345678
    ///     |d| d.serial_number() == Some("12345678"),
    ///     // Find the "Vendor Specific" interface
    ///     |i| i.class() == 0xFF,
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Use one-byte sequence numbers
    ///     VarSeqKind::Seq1,
    /// ).unwrap();
    /// ```
    #[cfg(not(target_os = "windows"))]
    pub fn try_new_raw_nusb_with_interface<
        F1: FnMut(&DeviceInfo) -> bool,
        F2: FnMut(&nusb::InterfaceInfo) -> bool,
    >(
        device_func: F1,
        interface_func: F2,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let x = nusb::list_devices()
            .map_err(|e| format!("Error listing devices: {e:?}"))?
            .find(device_func)
            .ok_or_else(|| String::from("Failed to find matching nusb device!"))?;
        let interface_id = x
            .interfaces()
            .position(interface_func)
            .ok_or_else(|| String::from("Failed to find matching interface!!"))?;

        Self::try_from_nusb_and_interface(
            &x,
            interface_id,
            err_uri_path,
            outgoing_depth,
            seq_no_kind,
        )
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
    /// // Assume the first usb device is the one we're interested
    /// let dev = nusb::list_devices().unwrap().next().unwrap();
    /// let client = HostClient::<Error>::try_from_nusb_and_interface(
    ///     // Device to open
    ///     &dev,
    ///     // Use the first interface (0)
    ///     0,
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Use one-byte sequence numbers
    ///     VarSeqKind::Seq1,
    /// ).unwrap();
    /// ```
    pub fn try_from_nusb_and_interface(
        dev: &DeviceInfo,
        interface_id: usize,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Result<Self, String> {
        let dev = _impl::open_device(dev).map_err(|e| format!("Failed opening device: {e:?}"))?;
        let interface = _impl::claim_interface(&dev, interface_id as u8)
            .map_err(|e| format!("Failed claiming interface: {e:?}"))?;

        let mut mps: Option<usize> = None;
        let mut ep_in: Option<u8> = None;
        let mut ep_out: Option<u8> = None;
        for ias in interface.descriptors() {
            for ep in ias.endpoints().filter(_impl::is_bulk_endpoint) {
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

        let tx = _impl::make_tx_impl(&interface, ep_out, mps)?;
        let rx = _impl::make_rx_impl(&interface, ep_out)?;

        Ok(HostClient::new_with_wire(
            tx,
            rx,
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
    /// let client = HostClient::<Error>::new_raw_nusb(
    ///     // Find the first device with the serial 12345678
    ///     |d| d.serial_number() == Some("12345678"),
    ///     // the URI/path for `Error` messages
    ///     "error",
    ///     // Outgoing queue depth in messages
    ///     8,
    ///     // Use one-byte sequence numbers
    ///     VarSeqKind::Seq1,
    /// );
    /// ```
    pub fn new_raw_nusb<F: FnMut(&DeviceInfo) -> bool>(
        func: F,
        err_uri_path: &str,
        outgoing_depth: usize,
        seq_no_kind: VarSeqKind,
    ) -> Self {
        Self::try_new_raw_nusb(func, err_uri_path, outgoing_depth, seq_no_kind)
            .expect("should have found nusb device")
    }
}

//////////////////////////////////////////////////////////////////////////////
// Wire Interface Implementation
//////////////////////////////////////////////////////////////////////////////

/// NUSB Wire Interface Implementor
///
/// Uses Tokio for spawning tasks
struct NusbSpawn;

impl WireSpawn for NusbSpawn {
    fn spawn(&mut self, fut: impl Future<Output = ()> + Send + 'static) {
        // Explicitly drop the joinhandle as it impls Future and this makes
        // clippy mad if you just let it drop implicitly
        core::mem::drop(tokio::task::spawn(fut));
    }
}
