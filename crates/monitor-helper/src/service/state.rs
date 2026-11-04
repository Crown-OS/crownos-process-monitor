use std::sync::Mutex;

use crate::HelperError;
use crate::auth::Authorizer;
use crate::fans::FanBoard;
use crate::lifetime::ClientRegistry;
use crate::sources::Sources;
use crate::util::lock::lock;

/// Everything the service and the exit loop share.
pub struct HelperState<A> {
    pub(crate) authorizer: A,
    pub(crate) clients: ClientRegistry,
    pub(crate) fans: Mutex<FanBoard>,
    pub(crate) sources: Sources,
}

impl<A: Authorizer> HelperState<A> {
    pub fn new(authorizer: A) -> Result<Self, HelperError> {
        Ok(Self {
            authorizer,
            clients: ClientRegistry::default(),
            fans: Mutex::default(),
            sources: Sources::open()?,
        })
    }

    pub fn clients(&self) -> &ClientRegistry {
        &self.clients
    }

    /// A client left the bus: forget its grants and hand its fans back to
    /// automatic control.
    pub fn client_departed(&self, bus_name: &str) -> Result<(), HelperError> {
        if !self.clients.departed(bus_name) {
            return Ok(());
        }
        self.authorizer.forget(bus_name);
        lock(&self.fans).restore_owned_by(bus_name)
    }

    pub fn restore_all_fans(&self) -> Result<(), HelperError> {
        lock(&self.fans).restore_all()
    }
}
