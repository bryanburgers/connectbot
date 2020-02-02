use chrono::Duration;
use chrono::{DateTime, Utc};
use std::collections::{hash_map::Entry, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use super::device_server::client_connection::ClientConnectionHandle;

use std;

mod connection_history;
pub use self::connection_history::{ConnectionHistory, ConnectionHistoryItem};
mod ssh_forward;
pub use self::ssh_forward::{
    SshForward, SshForwardClientState, SshForwardData, SshForwardServerState, SshForwards,
};
mod port_allocator;
pub use self::port_allocator::RemotePort;
use self::port_allocator::{PortAllocator, PortAllocatorSettings};

pub type SharedWorld = Arc<RwLock<World>>;

/// The state of the entire world
#[derive(Debug)]
pub struct World {
    /// Map from device ID to the device
    pub devices: HashMap<String, Device>,
    port_allocator: PortAllocator,
}

impl World {
    /// Create a new world
    pub fn new(config: super::config::SharedConfig) -> World {
        World {
            devices: HashMap::new(),
            port_allocator: PortAllocator::new(PortAllocatorSettings {
                web_start: config.ssh.web_port_start,
                web_end: config.ssh.web_port_end,
                other_start: config.ssh.port_start,
                other_end: config.ssh.port_end,
            }),
        }
    }

    /// Create a new world, wrapped in a shared lock
    pub fn shared(config: super::config::SharedConfig) -> SharedWorld {
        Arc::new(RwLock::new(World::new(config)))
    }

    /// Cleanup any old data that is no longer necessary
    pub fn cleanup(&mut self, now: DateTime<Utc>) {
        let connection_history_cutoff = now - Duration::days(2);
        let forwards_cutoff = now - Duration::hours(1);

        for device in self.devices.values_mut() {
            let active_connection = device.active_connection.clone();
            device.connection_history.cleanup(connection_history_cutoff);
            device
                .ssh_forwards
                .cleanup(now, forwards_cutoff, active_connection);
        }
    }

    /// Create a device manually
    ///
    /// Returns Ok(()) if the device was created successfully, or Err(()) if the device already
    /// exists. This method is useful for manually creating devices (e.g. via the ctrl tool)
    pub fn create_device(&mut self, id: &str) -> Result<(), ()> {
        let entry = self.devices.entry(id.to_string());

        match entry {
            std::collections::hash_map::Entry::Occupied(_) => Err(()),
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(Device::new(id, self.port_allocator.clone()));
                Ok(())
            }
        }
    }

    /// Mark the device with the given connection handle as connected.
    ///
    /// Returns the previous connection handle, if one existed. (This makes it possible to
    /// disconnect the previous connection handle, if necessary.)
    pub fn connect_device(
        &mut self,
        id: &str,
        handle: ClientConnectionHandle,
        address: &SocketAddr,
        connected_at: DateTime<Utc>,
    ) -> Option<ClientConnectionHandle> {
        let connection_id = handle.get_id();
        let port_allocator = self.port_allocator.clone();
        let device = self
            .devices
            .entry(id.to_string())
            .or_insert_with(|| Device::new(id, port_allocator));

        device.connection_status = ConnectionStatus::Connected {
            address: address.ip(),
        };
        device
            .connection_history
            .connect(connection_id, connected_at, address.ip());
        std::mem::replace(&mut device.active_connection, Some(handle))
    }

    /// Mark a device as disconnected
    pub fn disconnect_device(
        &mut self,
        device_id: &str,
        connection_id: usize,
        last_message: DateTime<Utc>,
    ) {
        let entry = self.devices.entry(device_id.to_string());
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
            if active_connection.get_id() == connection_id {
                device.connection_status = ConnectionStatus::Disconnected {
                    last_message: last_message,
                };
            }
        } else {
            // There is no active connection. Mark as disconnected.
            device.connection_status = ConnectionStatus::Disconnected {
                last_message: last_message,
            };
        }
        device.active_connection = None;
        device
            .connection_history
            .disconnect(connection_id, last_message);
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
    pub ssh_forwards: SshForwards,
    /// If there is an active connection for the device, a handle to that connection.
    pub active_connection: Option<ClientConnectionHandle>,
    /// Connection history
    pub connection_history: ConnectionHistory,
}

impl Device {
    fn new(id: &str, allocator: PortAllocator) -> Device {
        Device {
            id: id.to_owned(),
            name: id.to_owned(),
            connection_status: ConnectionStatus::Unknown,
            ssh_forwards: SshForwards::new(allocator),
            active_connection: None,
            connection_history: ConnectionHistory::new(),
        }
    }

    pub fn is_connected(&self) -> bool {
        match self.connection_status {
            ConnectionStatus::Connected { .. } => true,
            _ => false,
        }
    }
}

/// The connection status for a device
#[derive(Debug)]
pub enum ConnectionStatus {
    /// The client is currently connected
    Connected { address: IpAddr },
    /// The client has been connected, but is not currently connected
    Disconnected { last_message: DateTime<Utc> },
    /// The client has not been seen (or has not been seen recently)
    Unknown,
}
