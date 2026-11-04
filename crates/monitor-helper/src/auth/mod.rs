//! The authorization boundary. The service asks one question, "may this
//! caller use this capability?", and never learns which backend answered.
//! Swapping polkit for a CrownOS mechanism means adding a module here that
//! implements [`Authorizer`] and selecting it in [`selected`].

mod allow_list;
mod cache;
#[cfg(feature = "polkit")]
mod polkit;

use std::future::Future;

pub use allow_list::{AllowRule, StaticAuthorizer};
pub use cache::CachedAuthorizer;
#[cfg(feature = "polkit")]
pub use polkit::PolkitAuthorizer;

use crate::{Caller, Capability};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied,
    /// The backend would allow it after the person authenticates, but that
    /// did not happen (dismissed, or no agent available).
    NeedsAuthentication,
}

impl Decision {
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("authorization service failed: {0}")]
    Bus(#[from] zbus::Error),
    #[error("cannot identify caller: {0}")]
    Credentials(&'static str),
    #[error("invalid authorization policy: {0}")]
    Policy(String),
}

impl From<zbus::fdo::Error> for AuthError {
    fn from(error: zbus::fdo::Error) -> Self {
        Self::Bus(error.into())
    }
}

pub trait Authorizer: Send + Sync + 'static {
    fn authorize(
        &self,
        caller: &Caller,
        capability: Capability,
    ) -> impl Future<Output = Result<Decision, AuthError>> + Send;

    /// Drops anything remembered about a client that left the bus.
    fn forget(&self, _caller_bus_name: &str) {}
}

#[cfg(feature = "polkit")]
pub type SelectedAuthorizer = CachedAuthorizer<PolkitAuthorizer>;
#[cfg(not(feature = "polkit"))]
pub type SelectedAuthorizer = CachedAuthorizer<StaticAuthorizer>;

/// Builds the backend this binary was compiled with.
#[cfg(feature = "polkit")]
pub async fn selected(system_bus: &zbus::Connection) -> Result<SelectedAuthorizer, AuthError> {
    Ok(CachedAuthorizer::new(
        PolkitAuthorizer::connect(system_bus).await?,
    ))
}

#[cfg(not(feature = "polkit"))]
pub async fn selected(_system_bus: &zbus::Connection) -> Result<SelectedAuthorizer, AuthError> {
    Ok(CachedAuthorizer::new(StaticAuthorizer::load_default()?))
}
