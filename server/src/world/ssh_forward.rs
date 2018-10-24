use uuid;
use super::port_allocator::PortAllocator;
use super::port_allocator::RemotePort;

/// Information about a single ssh forwarding
#[derive(Debug)]
pub struct SshForward {
    pub id: String,
    pub client_state: SshForwardClientState,
    pub server_state: SshForwardServerState,
    pub forward_host: String,
    pub forward_port: u16,
    pub remote_port: Option<RemotePort>,
    pub gateway_port: bool,
}

impl SshForward {
    pub fn data(&self) -> SshForwardData {
        SshForwardData {
            id: self.id.clone(),
            forward_host: self.forward_host.clone(),
            forward_port: self.forward_port,
            remote_port: self.remote_port.as_ref().map_or(0, |item| item.value()),
            gateway_port: self.gateway_port,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SshForwardData {
    pub id: String,
    pub forward_host: String,
    pub forward_port: u16,
    pub remote_port: u16,
    pub gateway_port: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
pub struct SshForwards {
    forwards: Vec<SshForward>,
    allocator: super::port_allocator::PortAllocator,
}

impl SshForwards {
    pub fn new(allocator: PortAllocator) -> SshForwards {
        SshForwards {
            forwards: Vec::new(),
            allocator,
        }
    }

    pub fn find(&self, id: &str) -> Option<&SshForward> {
        for item in self.forwards.iter() {
            if item.id == id {
                return Some(item)
            }
        }

        None
    }

    pub fn iter(&self) -> ::std::slice::Iter<SshForward> {
        self.forwards.iter()
    }

    pub fn create(&mut self, forward_host: String, forward_port: u16, gateway_port: bool) -> &SshForward {
        let id = format!("{}", uuid::Uuid::new_v4());

        // TODO: Propagate error
        let remote_port = match forward_port {
            80 => self.allocator.allocate_web().expect("Ran out of ports"),
            _ => self.allocator.allocate().expect("Ran out of ports"),
        };

        let forward = SshForward {
            id: id.clone(),
            client_state: SshForwardClientState::Requested,
            server_state: SshForwardServerState::Active,
            forward_host,
            forward_port,
            remote_port: Some(remote_port),
            gateway_port,
        };

        self.forwards.push(forward);

        &self.forwards[self.forwards.len() - 1]
    }

    /// Update the current state of a client. Returns Ok(()) if the client was found, and Err(())
    /// if the client was not found.
    pub fn update_client_state(&mut self, id: &str, client_state: SshForwardClientState) -> Result<(), ()> {
        let mut success = false;
        for item in self.forwards.iter_mut() {
            if item.id == id {
                item.client_state = client_state;
                match item.client_state {
                    SshForwardClientState::Disconnected => {
                        item.remote_port = None;
                    },
                    _ => {},
                }
                success = true;
                break;
            }
        }

        match success {
            true => Ok(()),
            false => Err(()),
        }
    }

    pub fn disconnect(&mut self, id: &str) -> bool {
        let mut success = false;
        for item in self.forwards.iter_mut() {
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
        self.forwards.retain(|item| {
            match item {
                ConnectionHistoryItem::Closed { last_message, .. } if last_message < &cutoff => false,
                _ => true,
            }
        });
        */
    }
}
