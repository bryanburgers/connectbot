use std::collections::HashMap;
use std::sync::{RwLock, Arc};
use ssh_connection::{SshConnectionChange, SshConnectionHandle};

pub struct SshManager {
    state: Arc<RwLock<SshManagerState>>,
}

impl SshManager {
    pub fn new() -> SshManager {
        let state = Arc::new(RwLock::new(SshManagerState::new()));

        SshManager {
            state
        }
    }

    pub fn current_state(&self, id: &str) -> Option<SshConnectionChange> {
        let manager = self.state.read().unwrap();

        if let Some(connection) = manager.connections.get(&id.to_string()) {
            if let Some(ref last_change) = connection.last_change {
                return Some(last_change.clone());
            }
        }
        None
    }

    pub fn disable(&self, id: &str) {
        let manager = self.state.read().unwrap();

        if let Some(connection) = manager.connections.get(&id.to_string()) {
            if let Some(ref handle) = connection.handle {
                handle.disconnect();
            }
        }
    }

    pub fn get_ref(&self) -> SshManagerRef {
        SshManagerRef::new(self)
    }
}

pub struct SshManagerRef {
    state: Arc<RwLock<SshManagerState>>
}

impl SshManagerRef {
    fn new(manager: &SshManager) -> SshManagerRef {
        SshManagerRef {
            state: manager.state.clone()
        }
    }

    pub fn update_state(&self, id: &str, state: &SshConnectionChange) {
        let mut manager = self.state.write().unwrap();
        let item = manager.connections.entry(id.to_string())
            .or_insert_with(|| SshManagerItem { last_change: None, handle: None });

        item.last_change = Some(state.clone());
    }

    pub fn register_handle(&self, id: &str, handle: SshConnectionHandle) {
        let mut manager = self.state.write().unwrap();
        let item = manager.connections.entry(id.to_string())
            .or_insert_with(|| SshManagerItem { last_change: None, handle: None });

        item.handle = Some(handle);
    }
}

struct SshManagerState {
    connections: HashMap<String, SshManagerItem>
}

struct SshManagerItem {
    last_change: Option<SshConnectionChange>,
    handle: Option<SshConnectionHandle>,
}

impl SshManagerState {
    fn new() -> SshManagerState {
        let connections = HashMap::new();
        SshManagerState {
            connections
        }
    }
}
