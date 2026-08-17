//! Windows hook host for OpenViKey (GD2a).

pub mod classify;
pub mod focus;
pub mod hook;
pub mod host;
pub mod inject;
pub mod mouse;
pub mod overlay;
pub mod passphrase;
pub mod persist;
pub mod policy;
pub mod sync;
pub mod tray;

pub use persist::HostShutdown;
