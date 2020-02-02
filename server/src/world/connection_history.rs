use chrono::{DateTime, Utc};
use std::net::IpAddr;

/// A history of the times in recent memory that the device has been connected.
#[derive(Debug)]
pub struct ConnectionHistory(Vec<ConnectionHistoryItem>);

impl ConnectionHistory {
    /// Create a new connection history.
    pub fn new() -> ConnectionHistory {
        ConnectionHistory(Vec::new())
    }

    /// Get the underlying iterator.
    pub fn iter(&self) -> ::std::slice::Iter<ConnectionHistoryItem> {
        self.0.iter()
    }

    /// Mark a device as connected.
    pub fn connect(&mut self, connection_id: usize, connected_at: DateTime<Utc>, address: IpAddr) {
        self.0.push(ConnectionHistoryItem::Open {
            connection_id,
            connected_at,
            address,
        });
    }

    /// Mark a device as disconnected.
    pub fn disconnect(&mut self, connection_id: usize, last_message: DateTime<Utc>) {
        for i in 0..self.0.len() {
            let replace = match &self.0[i] {
                ConnectionHistoryItem::Open {
                    connection_id: ref conn_id,
                    ref connected_at,
                    ref address,
                } if connection_id == *conn_id => {
                    // Replace the existing open item with a closed item
                    Some(ConnectionHistoryItem::Closed {
                        connected_at: connected_at.clone(),
                        last_message,
                        address: address.clone(),
                    })
                }
                // Don't replace.
                _ => None,
            };

            if let Some(replace) = replace {
                self.0[i] = replace;
                break;
            }
        }
    }

    /// Get rid of stale information that is older than the cutoff.
    pub fn cleanup(&mut self, cutoff: DateTime<Utc>) {
        self.0.retain(|item| match item {
            ConnectionHistoryItem::Closed { last_message, .. } if last_message < &cutoff => false,
            _ => true,
        });
    }
}

/// A single connection history event
#[derive(Debug)]
pub enum ConnectionHistoryItem {
    /// A period of time in the past when the device was connected.
    Closed {
        connected_at: DateTime<Utc>,
        last_message: DateTime<Utc>,
        address: IpAddr,
    },
    /// The device is currently connected, and that connection started at the given time.
    Open {
        connection_id: usize,
        connected_at: DateTime<Utc>,
        address: IpAddr,
    },
}
