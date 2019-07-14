use chrono::{TimeZone, Utc};
use connectbot_shared::protos::control;
use serde_derive::Serialize;

/// Information about a single device
#[derive(Serialize, Debug)]
pub struct Device {
    /// The unique ID of the device
    pub id: String,
    /// A user-specified name of the device
    pub name: String,
    /// The IP address (either IPv4 or IPv6) of the device
    pub address: Option<String>,
    /// A list of known connections (SSH port forwards)
    pub connections: Vec<DeviceConnection>,
    /// The history of when the device has been connected and disconnected
    pub connection_history: Vec<DeviceHistoryItem>,
}

impl From<control::ClientsResponse_Client> for Device {
    fn from(mut client: control::ClientsResponse_Client) -> Self {
        let id = client.take_id().to_string();
        let name = client.take_name().to_string();
        let connection_history = client.take_connection_history()
            .into_iter()
            .map(Into::into)
            .collect();

        let connections = client.take_connections()
            .into_iter()
            .map(Into::into)
            .collect();

        let address = client.take_address().to_string();
        let address = if address == "" {
            None
        }
        else {
            Some(address)
        };

        Device {
            name: name,
            id: id,
            address,
            connections,
            connection_history,
        }
    }
}

/// Information about when a device was connected
#[derive(Serialize, Debug)]
pub struct DeviceHistoryItem {
    /// The time at which the device was connected. Note that this is a string because this
    /// structure is used to serialize to JSON, so we don't need it stored as a date.
    pub connected_at: String,
    /// The time at which the device last sent a message to the server.
    pub last_message: Option<String>,
    /// The IP address of this particular connection.
    pub address: String,
}

impl From<control::ClientsResponse_ConnectionHistoryItem> for DeviceHistoryItem {
    fn from(client: control::ClientsResponse_ConnectionHistoryItem) -> Self {
        let history_type = client.get_field_type();
        let connected_at = Utc.timestamp(client.get_connected_at() as i64, 0);
        let last_message = Utc.timestamp(client.get_last_message() as i64, 0);
        let address = client.get_address().to_string();

        match history_type {
            control::ClientsResponse_ConnectionHistoryType::UNKNOWN_CONNECTION_HISTORY_TYPE => {
                panic!("Invalid response!");
            },
            control::ClientsResponse_ConnectionHistoryType::CLOSED => {
                DeviceHistoryItem {
                    connected_at: connected_at.to_rfc3339(),
                    last_message: Some(last_message.to_rfc3339()),
                    address,
                }
            },
            control::ClientsResponse_ConnectionHistoryType::OPEN => {
                DeviceHistoryItem {
                    connected_at: connected_at.to_rfc3339(),
                    last_message: None,
                    address,
                }
            },
        }
    }
}

/// Information about an SSH connection
#[derive(Serialize, Debug)]
pub struct DeviceConnection {
    /// The unique ID of the connection itself
    pub id: String,
    /// The state of the connection, according to the device
    pub client_state: DeviceConnectionClientState,
    /// Whether the connection is currently active or inactive
    pub active_state: DeviceConnectionActiveState,
    /// The port allocated to this connection
    pub remote_port: u16,
    /// The port on the client (or the device near the client) that is being forwarded
    pub forward_port: u16,
    /// The host on the client network that is being forwarded
    pub forward_host: String,
    /// Whether the connection is an HTTP connection
    pub is_http: bool,
    /// Whether the connection is a reverse SSH connection
    pub is_ssh: bool,
    /// The time at which the connection will expire
    pub active_until: Option<String>,
}

/// The state of the connection, according to the device.
#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DeviceConnectionClientState {
    Requested,
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
    Failed,
}

/// Information about whether the connection is currently active or not.
#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DeviceConnectionActiveState {
    Active,
    Inactive,
}

impl From<control::ClientsResponse_Connection> for DeviceConnection {
    fn from(connection: control::ClientsResponse_Connection) -> Self {
        let active_until = if connection.get_active_until() == 0 {
            None
        }
        else {
            Some(Utc.timestamp(connection.get_active_until() as i64, 0).to_rfc3339())
        };

        DeviceConnection {
            id: connection.get_id().to_string(),
            client_state: match connection.get_state() {
                control::ClientsResponse_ClientState::UNKNOWN_STATE => DeviceConnectionClientState::Failed,
                control::ClientsResponse_ClientState::REQUESTED => DeviceConnectionClientState::Requested,
                control::ClientsResponse_ClientState::CONNECTING => DeviceConnectionClientState::Connecting,
                control::ClientsResponse_ClientState::CONNECTED => DeviceConnectionClientState::Connected,
                control::ClientsResponse_ClientState::DISCONNECTING => DeviceConnectionClientState::Disconnecting,
                control::ClientsResponse_ClientState::DISCONNECTED => DeviceConnectionClientState::Disconnected,
                control::ClientsResponse_ClientState::FAILED => DeviceConnectionClientState::Failed,
            },
            active_state: match connection.get_active() {
                control::ClientsResponse_ActiveState::UNKNOWN_ACTIVE_STATE => DeviceConnectionActiveState::Inactive,
                control::ClientsResponse_ActiveState::ACTIVE => DeviceConnectionActiveState::Active,
                control::ClientsResponse_ActiveState::INACTIVE => DeviceConnectionActiveState::Inactive,
            },
            forward_port: connection.get_forward_port() as u16,
            forward_host: connection.get_forward_host().to_string(),
            remote_port: connection.get_remote_port() as u16,
            is_http: connection.get_forward_port() == 80,
            is_ssh: connection.get_forward_port() == 22,
            active_until,
        }
    }
}
