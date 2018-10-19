use std::net::IpAddr;
use chrono::{DateTime, Utc};

/// A history of the times in recent memory that the device has been connected.
#[derive(Debug)]
pub struct ConnectionHistory(Vec<ConnectionHistoryItem>);

impl ConnectionHistory {
    pub fn new() -> ConnectionHistory {
        ConnectionHistory(Vec::new())
    }

    pub fn iter(&self) -> ::std::slice::Iter<ConnectionHistoryItem> {
        self.0.iter()
    }

    pub fn connect(&mut self, connection_id: usize, connected_at: DateTime<Utc>, address: IpAddr) {
        self.0.push(ConnectionHistoryItem::Open { connection_id, connected_at, address });
    }

    pub fn disconnect(&mut self, connection_id: usize, last_message: DateTime<Utc>) {
        for i in 0..self.0.len() {
            let replace = match &self.0[i] {
                ConnectionHistoryItem::Open { connection_id: ref conn_id, ref connected_at, ref address } if connection_id == *conn_id => {
                    // Replace the existing open item with a closed item
                    Some(ConnectionHistoryItem::Closed { connected_at: connected_at.clone(), last_message, address: address.clone() })
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
    Closed { connected_at: DateTime<Utc>, last_message: DateTime<Utc>, address: IpAddr },
    /// The device is currently connected, and that connection started at the given time.
    Open { connection_id: usize, connected_at: DateTime<Utc>, address: IpAddr },
}
