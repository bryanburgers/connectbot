use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use std::net::IpAddr;

pub type SharedWorld = Arc<RwLock<World>>;

/// The state of the entire world
#[derive(Debug)]
pub struct World {
    /// Map from device ID to the device
    pub devices: HashMap<String, Device>,
}

impl World {
    /// Create a new world
    pub fn new() -> World {
        World {
            devices: HashMap::new(),
        }
    }

    /// Create a new world, wrapped in a shared lock
    pub fn shared() -> SharedWorld {
        Arc::new(RwLock::new(World::new()))
    }
}

/// Information about a single device
#[derive(Debug)]
pub struct Device {
    /// The unique ID of the device. This is typically a uuid
    pub id: String,
    /// The human-readable name of the device (maybe)
    pub name: String,
    /// Whether the device is currently connected
    pub connection_status: ConnectionStatus,
    /// Information about forwards for the current device
    pub ssh_forwards: HashMap<String, SshForward>,
}

impl Device {
    pub fn new(id: &str) -> Device {
        Device {
            id: id.to_owned(),
            name: id.to_owned(),
            connection_status: ConnectionStatus::Unknown,
            ssh_forwards: HashMap::new(),
        }
    }
}

/// Information about a single ssh forwarding
#[derive(Debug)]
pub struct SshForward {
    pub id: String,
}

/// The connection status for a device
#[derive(Debug)]
pub enum ConnectionStatus {
    /// The client is currently connected
    Connected { address: IpAddr },
    /// The client has been connected, but is not currently connected
    Disconnected { last_seen: Instant },
    /// The client has not been seen (or has not been seen recently)
    Unknown,
}
