use std::sync::{Arc, RwLock};

/// A struct that hands out open ports for connections to use, and ensures that only one connection
/// ever has that port at any given time.
#[derive(Debug, Clone)]
pub struct PortAllocator {
    port_allocator: Arc<PrivatePortAllocator>
}

/// Failure reasons for why we can't hand out a port
#[derive(Clone, Debug)]
pub enum PortAllocationError {
    /// All of the ports are taken
    NoAvailablePorts,
}

/// The type of port that is requested or handed out
#[derive(Debug)]
enum PortType {
    /// The port is a web port. This distinction exists because we can use nginx to set up proxies
    /// to web ports and forward the HTTP traffic
    Web,
    /// The port is, well, not a web port. Any other type of port.
    Other,
}

/// Settings from the configuration file about which port ranges to use for web ports and all other
/// ports.
pub struct PortAllocatorSettings {
    /// The first port to use for web ports
    pub web_start: u16,
    /// The last port (inclusive) to use for web ports
    pub web_end: u16,
    /// The first port ot use for other ports
    pub other_start: u16,
    /// The last port (inclusive) to use for other ports
    pub other_end: u16,
}

impl PortAllocator {
    /// Create a new port allocator
    pub fn new(settings: PortAllocatorSettings) -> PortAllocator {
        PortAllocator {
            port_allocator: Arc::new(PrivatePortAllocator::new(settings))
        }
    }

    /// Try to allocate a single web port
    pub fn allocate_web(&self) -> Result<RemotePort, PortAllocationError> {
        self.port_allocator.allocate(PortType::Web, self.port_allocator.clone())
    }

    /// Try to allocate a single other port
    pub fn allocate(&self) -> Result<RemotePort, PortAllocationError> {
        self.port_allocator.allocate(PortType::Other, self.port_allocator.clone())
    }
}

/// Internal data about the port allocator. PortAllocator itself stores an Arc to this, so that it
/// can be cloned easily. This is the real workhorse.
#[derive(Debug)]
struct PrivatePortAllocator {
    web_port_range: RwLock<ReservablePortRange>,
    other_port_range: RwLock<ReservablePortRange>,
}

impl PrivatePortAllocator {
    /// Create a new one.
    pub fn new(settings: PortAllocatorSettings) -> PrivatePortAllocator {
        PrivatePortAllocator {
            web_port_range: RwLock::new(ReservablePortRange::new(settings.web_start, settings.web_end)),
            other_port_range: RwLock::new(ReservablePortRange::new(settings.other_start, settings.other_end)),
        }
    }

    /// Allocate a single port, if possible.
    pub fn allocate(&self, port_type: PortType, allocator: Arc<Self>) -> Result<RemotePort, PortAllocationError> {
        let next = match port_type {
            PortType::Web => self.web_port_range.write().unwrap().take_next(),
            PortType::Other => self.other_port_range.write().unwrap().take_next(),
        };
        match next {
            Some(port) => {
                Ok(RemotePort { port_value: port, port_type, deallocator: allocator })
            },
            None => Err(PortAllocationError::NoAvailablePorts),
        }
    }

    /// Return the port. Note that clients don't need to call this. The RemotePort calls this on
    /// drop.
    pub fn deallocate(&self, port: &RemotePort) {
        match port.port_type {
            PortType::Web => self.web_port_range.write().unwrap().return_port(port.port_value),
            PortType::Other => self.other_port_range.write().unwrap().return_port(port.port_value),
        }
    }
}

/// The workhorse for a single range of ports.
#[derive(Debug)]
struct ReservablePortRange {
    /// The first port that we're allowed to hand out
    start: u16,
    /// The last port that we're allowed to hand out
    end: u16,
    /// The next port that we'll hand out
    next: usize,
    /// Which ports we have and haven't handed out
    vec: Vec<bool>
}

impl ReservablePortRange {
    /// Create a new one
    fn new(start: u16, end: u16) -> ReservablePortRange {
        assert!(end >= start, "end must be greater than start");
        let size = (end - start + 1) as usize;
        let mut vec = Vec::with_capacity(size);
        vec.resize(size, false);
        ReservablePortRange {
            start,
            end,
            next: 0,
            vec,
        }
    }

    /// Figure out which is the next port that can be handed out, mark it as handed out, and return
    /// it. Returns None if there are none left.
    fn take_next(&mut self) -> Option<u16> {
        let original_next = self.next;
        let size = (self.end - self.start + 1) as usize;

        for i in original_next..size {
            if self.vec[i] == false {
                self.vec[i] = true;
                self.next = (i + 1) % size;

                return Some((i as u16) + self.start)
            }
        }
        for i in 0..original_next {
            if self.vec[i] == false {
                self.vec[i] = true;
                self.next = (i + 1) % size;

                return Some((i as u16) + self.start)
            }
        }

        None
    }

    /// Return a port back to the pool.
    fn return_port(&mut self, port: u16) {
        assert!(port >= self.start, "port must be within the port range");
        assert!(port <= self.end, "port must be within the port range");
        let index = port - self.start;
        self.vec[index as usize] = false;
    }
}

/// An allocated remote port. The caller must hold a reference to this as long as it is using the
/// remote port. Once it goes out of scope, the port gets returned to the pool to be used again.
#[derive(Debug)]
pub struct RemotePort {
    port_value: u16,
    port_type: PortType,
    deallocator: Arc<PrivatePortAllocator>
}

impl Drop for RemotePort {
    fn drop(&mut self) {
        // Return the port back to the PortAllocator!
        self.deallocator.deallocate(self);
    }
}

impl RemotePort {
    /// Get the actual port number from the RemotePort object
    pub fn value(&self) -> u16 {
        self.port_value
    }
}
