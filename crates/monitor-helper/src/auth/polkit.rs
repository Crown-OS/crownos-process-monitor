use std::collections::HashMap;

use zbus::Variant;

use super::{AuthError, Authorizer, Decision};
use crate::{Caller, Capability};

const ALLOW_USER_INTERACTION: u32 = 1;
const NO_INTERACTION: u32 = 0;
const UNIX_PROCESS_SUBJECT: &str = "unix-process";

#[zbus::proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait Authority {
    fn check_authorization(
        &self,
        subject: &(&str, HashMap<&str, Variant<'_>>),
        action_id: &str,
        details: HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<(bool, bool, HashMap<String, String>)>;
}

pub struct PolkitAuthorizer {
    authority: AuthorityProxy<'static>,
}

impl PolkitAuthorizer {
    pub async fn connect(system_bus: &zbus::Connection) -> Result<Self, AuthError> {
        Ok(Self {
            authority: AuthorityProxy::new(system_bus).await?,
        })
    }
}

/// The actions declared in `data/org.crownos.process-monitor.policy`.
const fn action_id(capability: Capability) -> &'static str {
    match capability {
        Capability::ReadAllProcesses
        | Capability::ReadEnergy
        | Capability::ReadNetworkAccounting
        | Capability::ReadGpuCounters => "org.crownos.process-monitor.read-metrics",
        Capability::ControlFans => "org.crownos.process-monitor.control-fans",
        Capability::SignalOtherUsers => "org.crownos.process-monitor.signal-other-users",
    }
}

impl Authorizer for PolkitAuthorizer {
    async fn authorize(
        &self,
        caller: &Caller,
        capability: Capability,
    ) -> Result<Decision, AuthError> {
        let uid = i32::try_from(caller.uid)
            .map_err(|_| AuthError::Credentials("user id out of range"))?;
        let subject_details = HashMap::from([
            ("pid", Variant::from(caller.pid)),
            ("start-time", Variant::from(caller.start_ticks)),
            ("uid", Variant::from(uid)),
        ]);
        let flags = if capability.is_read_only() {
            NO_INTERACTION
        } else {
            ALLOW_USER_INTERACTION
        };
        let (is_authorized, is_challenge, _) = self
            .authority
            .check_authorization(
                &(UNIX_PROCESS_SUBJECT, subject_details),
                action_id(capability),
                HashMap::new(),
                flags,
                "",
            )
            .await?;
        Ok(match (is_authorized, is_challenge) {
            (true, _) => Decision::Allowed,
            (false, true) => Decision::NeedsAuthentication,
            (false, false) => Decision::Denied,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_share_one_quiet_action() {
        let read_actions: Vec<_> = Capability::ALL
            .into_iter()
            .filter(|capability| capability.is_read_only())
            .map(action_id)
            .collect();
        assert!(read_actions.windows(2).all(|pair| pair[0] == pair[1]));
        assert_ne!(
            action_id(Capability::ControlFans),
            action_id(Capability::SignalOtherUsers)
        );
    }
}
