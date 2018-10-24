use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct PortAllocator {
    port_allocator: Arc<PrivatePortAllocator>
}

#[derive(Clone, Debug)]
pub enum PortAllocationError {
    NoAvailablePorts,
}

#[derive(Debug)]
enum PortType {
    Web,
    Other,
}

pub struct PortAllocatorSettings {
    pub web_start: u16,
    pub web_end: u16,
    pub other_start: u16,
    pub other_end: u16,
}

impl PortAllocator {
    pub fn new(settings: PortAllocatorSettings) -> PortAllocator {
        PortAllocator {
            port_allocator: Arc::new(PrivatePortAllocator::new(settings))
        }
    }

    pub fn allocate_web(&self) -> Result<RemotePort, PortAllocationError> {
        self.port_allocator.allocate(PortType::Web, self.port_allocator.clone())
    }

    pub fn allocate(&self) -> Result<RemotePort, PortAllocationError> {
        self.port_allocator.allocate(PortType::Other, self.port_allocator.clone())
    }
}

#[derive(Debug)]
struct PrivatePortAllocator {
    web_port_range: RwLock<ReservablePortRange>,
    other_port_range: RwLock<ReservablePortRange>,
}

impl PrivatePortAllocator {
    pub fn new(settings: PortAllocatorSettings) -> PrivatePortAllocator {
        PrivatePortAllocator {
            web_port_range: RwLock::new(ReservablePortRange::new(settings.web_start, settings.web_end)),
            other_port_range: RwLock::new(ReservablePortRange::new(settings.other_start, settings.other_end)),
        }
    }

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

    pub fn deallocate(&self, port: &RemotePort) {
        match port.port_type {
            PortType::Web => self.web_port_range.write().unwrap().return_port(port.port_value),
            PortType::Other => self.other_port_range.write().unwrap().return_port(port.port_value),
        }
    }
}

#[derive(Debug)]
struct ReservablePortRange {
    start: u16,
    end: u16,
    next: usize,
    vec: Vec<bool>
}

impl ReservablePortRange {
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

    fn return_port(&mut self, port: u16) {
        assert!(port >= self.start, "port must be within the port range");
        assert!(port <= self.end, "port must be within the port range");
        let index = port - self.start;
        self.vec[index as usize] = false;
    }
}

#[derive(Debug)]
pub struct RemotePort {
    port_value: u16,
    port_type: PortType,
    deallocator: Arc<PrivatePortAllocator>
}

impl Drop for RemotePort {
    fn drop(&mut self) {
        self.deallocator.deallocate(self);
    }
}

impl RemotePort {
    pub fn value(&self) -> u16 {
        self.port_value
    }
}
