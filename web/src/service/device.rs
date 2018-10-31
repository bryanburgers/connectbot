use chrono::{TimeZone, Utc};
use connectbot_shared::protos::control;
use serde_derive::Serialize;

#[derive(Serialize, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub address: Option<String>,
    pub connections: Vec<DeviceConnection>,
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

#[derive(Serialize, Debug)]
pub struct DeviceHistoryItem {
    pub connected_at: String,
    pub last_message: Option<String>,
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

#[derive(Serialize, Debug)]
pub struct DeviceConnection {
    pub id: String,
    pub client_state: DeviceConnectionClientState,
    pub active_state: DeviceConnectionActiveState,
    pub remote_port: u16,
    pub forward_port: u16,
    pub forward_host: String,
}

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

#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DeviceConnectionActiveState {
    Active,
    Inactive,
}

impl From<control::ClientsResponse_Connection> for DeviceConnection {
    fn from(connection: control::ClientsResponse_Connection) -> Self {
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
        }
    }
}
