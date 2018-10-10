use std::collections::{HashMap, hash_map::Entry};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use std::net::{IpAddr, SocketAddr};

use super::server::client_connection::ClientConnectionHandle;

use std;

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

    pub fn connect_device(&mut self, id: &str, handle: ClientConnectionHandle, address: &SocketAddr) -> Option<ClientConnectionHandle> {
        let device = self.devices.entry(id.to_string())
            .or_insert_with(|| {
                Device::new(id)
            });

        device.connection_status = ConnectionStatus::Connected { address: address.ip() };
        std::mem::replace(&mut device.active_connection, Some(handle))
    }

    pub fn disconnect_device(&mut self, id: &str, last_message: Instant) {
        let entry = self.devices.entry(id.to_string());
        let entry = match entry {
            // We're disconnecting an already connected device. We definitely expect there to be an
            // entry in our world.
            Entry::Vacant(_) => panic!("Unexpected vacant entry!"),
            Entry::Occupied(o) => o,
        };

        let device = entry.into_mut();
        if let Some(ref active_connection) = device.active_connection {
            // If there is an active connection, only disconnect if the active
            // connection is the current connection. Otherwise, our connection was
            // replaced by a different connection, and so we don't want to replace THAT
            // connection status with disconnected.
            if active_connection.get_id() == id {
                device.connection_status = ConnectionStatus::Disconnected { last_seen: last_message };
            }
        }
        else {
            // There is no active connection. Mark as disconnected.
            device.connection_status = ConnectionStatus::Disconnected { last_seen: last_message };
        }
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
    /// If there is an active connection for the device, a handle to that connection.
    pub active_connection: Option<ClientConnectionHandle>,
}

impl Device {
    pub fn new(id: &str) -> Device {
        Device {
            id: id.to_owned(),
            name: id.to_owned(),
            connection_status: ConnectionStatus::Unknown,
            ssh_forwards: HashMap::new(),
            active_connection: None,
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
