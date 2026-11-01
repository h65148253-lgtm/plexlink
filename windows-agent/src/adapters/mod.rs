pub mod discovery;
pub mod monitoring;
pub mod interface_binding;

pub use discovery::{discover_adapters, InterfaceKind, NetworkAdapter, OperStatus};
pub use monitoring::{InterfaceChange, InterfaceEvent, InterfaceHealth, InterfaceMonitor};
