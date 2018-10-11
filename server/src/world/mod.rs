use std::collections::{HashMap, hash_map::Entry};
use std::sync::{Arc, RwLock};
use std::net::{IpAddr, SocketAddr};
use chrono::{DateTime, Utc};
use chrono::Duration;

use super::device_server::client_connection::ClientConnectionHandle;

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

    /// Cleanup any old data that is no longer necessary
    pub fn cleanup(&mut self, now: DateTime<Utc>) {
        let connection_history_cutoff = now - Duration::days(2);

        for mut device in self.devices.values_mut() {
            device.connection_history.cleanup(connection_history_cutoff);
        }
    }

    /// Mark the device with the given connection handle as connected.
    ///
    /// Returns the previous connection handle, if one existed. (This makes it possible to
    /// disconnect the previous connection handle, if necessary.)
    pub fn connect_device(&mut self, id: &str, handle: ClientConnectionHandle, address: &SocketAddr, connected_at: DateTime<Utc>) -> Option<ClientConnectionHandle> {
        let connection_id = handle.get_id();
        let device = self.devices.entry(id.to_string())
            .or_insert_with(|| {
                Device::new(id)
            });

        device.connection_status = ConnectionStatus::Connected { address: address.ip() };
        device.connection_history.connect(connection_id, connected_at);
        std::mem::replace(&mut device.active_connection, Some(handle))
    }

    /// Mark a device as disconnected
    pub fn disconnect_device(&mut self, device_id: &str, connection_id: usize, last_message: DateTime<Utc>) {
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
                device.connection_status = ConnectionStatus::Disconnected { last_message: last_message };
            }
        }
        else {
            // There is no active connection. Mark as disconnected.
            device.connection_status = ConnectionStatus::Disconnected { last_message: last_message };
        }
        device.connection_history.disconnect(connection_id, last_message);
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
    /// Connection history
    pub connection_history: ConnectionHistory,
}

impl Device {
    fn new(id: &str) -> Device {
        Device {
            id: id.to_owned(),
            name: id.to_owned(),
            connection_status: ConnectionStatus::Unknown,
            ssh_forwards: HashMap::new(),
            active_connection: None,
            connection_history: ConnectionHistory::new(),
        }
    }
}

/// A history of the times in recent memory that the device has been connected.
#[derive(Debug)]
pub struct ConnectionHistory(Vec<ConnectionHistoryItem>);

impl ConnectionHistory {
    pub fn new() -> ConnectionHistory {
        ConnectionHistory(Vec::new())
    }

    pub fn connect(&mut self, connection_id: usize, connected_at: DateTime<Utc>) {
        self.0.push(ConnectionHistoryItem::Open { connection_id, connected_at });
    }

    pub fn disconnect(&mut self, connection_id: usize, last_message: DateTime<Utc>) {
        for i in 0..self.0.len() {
            let replace = match &self.0[i] {
                ConnectionHistoryItem::Open { connection_id: ref conn_id, ref connected_at } if connection_id == *conn_id => {
                    // Replace the existing open item with a closed item
                    Some(ConnectionHistoryItem::Closed { connected_at: connected_at.clone(), last_message })
                },
                // Don't replace.
                _ => None,
            };

            if let Some(replace) = replace {
                self.0[i] = replace;
                break;
            }
        }
    }

    pub fn cleanup(&mut self, cutoff: DateTime<Utc>) {
        self.0.retain(|item| {
            match item {
                ConnectionHistoryItem::Closed { last_message, .. } if last_message < &cutoff => false,
                _ => true,
            }
        });
    }
}

/// A single connection history event
#[derive(Debug)]
pub enum ConnectionHistoryItem {
    /// A period of time in the past when the device was connected.
    Closed { connected_at: DateTime<Utc>, last_message: DateTime<Utc> },
    /// The device is currently connected, and that connection started at the given time.
    Open { connection_id: usize, connected_at: DateTime<Utc> },
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
    Disconnected { last_message: DateTime<Utc> },
    /// The client has not been seen (or has not been seen recently)
    Unknown,
}
