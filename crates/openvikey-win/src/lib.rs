//! Windows hook host for OpenViKey (GD2a).

pub mod classify;
pub mod console;
pub mod context_bridge;
pub mod focus;
pub mod hook;
pub mod host;
pub mod inject;
pub mod ll;
pub mod mouse;
pub mod overlay;
pub mod persist;
pub mod policy;
pub mod sync;
pub mod tray;

pub use persist::HostShutdown;
