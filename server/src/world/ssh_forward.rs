use uuid;

/// Information about a single ssh forwarding
#[derive(Debug, Clone)]
pub struct SshForward {
    pub id: String,
    pub client_state: SshForwardClientState,
    pub server_state: SshForwardServerState,
    pub forward_host: String,
    pub forward_port: u16,
    pub remote_port: u16,
    pub gateway_port: bool,
}

#[derive(Debug, Clone)]
pub enum SshForwardServerState {
    // TODO: Active until...
    /// The server is actively attempting to keep the device connected
    Active,
    // TODO: Inactive since?
    /// The server will no longer keep the device connected
    Inactive,
}

/// The state of the connection, as the client has reported it to us.
#[derive(Debug, Clone)]
pub enum SshForwardClientState {
    /// The client has not reported any state to us.
    Requested,
    /// The client has reported that it received our request and is actively connecting.
    Connecting,
    /// The client has reported that the SSH session is connected.
    Connected,
    /// The client has reported that it has received our request to disconnect, and is actively
    /// disconnecting.
    Disconnecting,
    /// The client has reported that it has disconnected the session.
    Disconnected,
    /// The client has reported that it tried and failed to establish an SSH session.
    Failed,
}

/// All of the SSH forwards for a device
#[derive(Debug)]
pub struct SshForwards(Vec<SshForward>);

impl SshForwards {
    pub fn new() -> SshForwards {
        SshForwards(Vec::new())
    }

    pub fn find(&self, id: &str) -> Option<&SshForward> {
        for item in self.0.iter() {
            if item.id == id {
                return Some(item)
            }
        }

        None
    }

    pub fn iter(&self) -> ::std::slice::Iter<SshForward> {
        self.0.iter()
    }

    pub fn create(&mut self, forward_host: String, forward_port: u16, remote_port: u16, gateway_port: bool) -> &SshForward {
        let id = format!("{}", uuid::Uuid::new_v4());

        let forward = SshForward {
            id: id.clone(),
            client_state: SshForwardClientState::Requested,
            server_state: SshForwardServerState::Active,
            forward_host,
            forward_port,
            remote_port,
            gateway_port,
        };

        self.0.push(forward);

        &self.0[self.0.len() - 1]
    }

    pub fn disconnect(&mut self, id: &str) -> bool {
        let mut success = false;
        for item in self.0.iter_mut() {
            if item.id == id {
                item.server_state = SshForwardServerState::Inactive;
                success = true;
                break;
            }
        }
        success
    }

    pub fn cleanup(&mut self) {
        /*
        self.0.retain(|item| {
            match item {
                ConnectionHistoryItem::Closed { last_message, .. } if last_message < &cutoff => false,
                _ => true,
            }
        });
        */
    }
}
