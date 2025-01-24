//! Implementations of various Server traits
//!
//! The implementations in this module typically require feature flags to be set.

#[cfg(feature = "embassy-usb-0_3-server")]
pub mod embassy_usb_v0_3;

#[cfg(feature = "embassy-usb-0_4-server")]
pub mod embassy_usb_v0_4;

#[cfg(feature = "test-utils")]
pub mod test_channels;
