//! A policy file backend for systems without polkit, development and tests.
//!
//! Format, one rule per line: `<capability> = <who>, <who>, …` where `<who>`
//! is `*`, `uid:<n>` or `gid:<n>`. Lines starting with `#` are ignored. A
//! capability without a rule is denied.

use std::collections::HashMap;
use std::path::Path;

use super::{AuthError, Authorizer, Decision};
use crate::{Caller, Capability};

const DEFAULT_POLICY_PATH: &str = "/etc/crownos/process-monitor-helper.rules";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AllowRule {
    pub everyone: bool,
    pub uids: Vec<u32>,
    pub gids: Vec<u32>,
}

impl AllowRule {
    fn admits(&self, caller: &Caller) -> bool {
        self.everyone
            || self.uids.contains(&caller.uid)
            || caller
                .supplementary_groups
                .iter()
                .any(|gid| self.gids.contains(gid))
    }
}

#[derive(Clone, Debug, Default)]
pub struct StaticAuthorizer {
    rules: HashMap<Capability, AllowRule>,
}

impl StaticAuthorizer {
    pub fn new(rules: HashMap<Capability, AllowRule>) -> Self {
        Self { rules }
    }

    pub fn load_default() -> Result<Self, AuthError> {
        Self::load(Path::new(DEFAULT_POLICY_PATH))
    }

    pub fn load(path: &Path) -> Result<Self, AuthError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| AuthError::Policy(format!("{}: {error}", path.display())))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, AuthError> {
        let rules = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(parse_rule)
            .collect::<Result<_, _>>()?;
        Ok(Self { rules })
    }
}

fn parse_rule(line: &str) -> Result<(Capability, AllowRule), AuthError> {
    let (name, members) = line
        .split_once('=')
        .ok_or_else(|| AuthError::Policy(format!("missing '=' in {line:?}")))?;
    let capability = Capability::from_name(name.trim())
        .ok_or_else(|| AuthError::Policy(format!("unknown capability {:?}", name.trim())))?;
    let mut rule = AllowRule::default();
    for member in members
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
    {
        match member.split_once(':') {
            None if member == "*" => rule.everyone = true,
            Some(("uid", id)) => rule.uids.push(parse_id(id)?),
            Some(("gid", id)) => rule.gids.push(parse_id(id)?),
            _ => return Err(AuthError::Policy(format!("unknown member {member:?}"))),
        }
    }
    Ok((capability, rule))
}

fn parse_id(id: &str) -> Result<u32, AuthError> {
    id.trim()
        .parse()
        .map_err(|_| AuthError::Policy(format!("invalid id {id:?}")))
}

impl Authorizer for StaticAuthorizer {
    async fn authorize(
        &self,
        caller: &Caller,
        capability: Capability,
    ) -> Result<Decision, AuthError> {
        let admitted = self
            .rules
            .get(&capability)
            .is_some_and(|rule| rule.admits(caller));
        Ok(if admitted {
            Decision::Allowed
        } else {
            Decision::Denied
        })
    }
}

#[cfg(test)]
mod tests {
    use zbus::names::OwnedUniqueName;

    use super::*;

    const POLICY: &str = "
        # reads for everyone, fans for wheel, signals for root only
        read-energy = *
        control-fans = gid:998
        signal-other-users = uid:0
    ";

    fn caller(uid: u32, groups: &[u32]) -> Option<Caller> {
        Some(Caller {
            bus_name: OwnedUniqueName::try_from(":1.1").ok()?,
            uid,
            pid: 1,
            start_ticks: 1,
            supplementary_groups: groups.to_vec(),
            pidfd: None,
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn applies_rules() {
        let Ok(policy) = StaticAuthorizer::parse(POLICY) else {
            panic!("policy should parse");
        };
        let Some(user) = caller(1000, &[998]) else {
            return;
        };
        let Some(stranger) = caller(1001, &[]) else {
            return;
        };
        let decide = |caller, capability| policy.authorize(caller, capability);
        assert!(matches!(
            decide(&user, Capability::ReadEnergy).await,
            Ok(Decision::Allowed)
        ));
        assert!(matches!(
            decide(&user, Capability::ControlFans).await,
            Ok(Decision::Allowed)
        ));
        assert!(matches!(
            decide(&stranger, Capability::ControlFans).await,
            Ok(Decision::Denied)
        ));
        assert!(matches!(
            decide(&user, Capability::SignalOtherUsers).await,
            Ok(Decision::Denied)
        ));
        assert!(matches!(
            decide(&user, Capability::ReadGpuCounters).await,
            Ok(Decision::Denied)
        ));
    }

    #[test]
    fn rejects_malformed_policies() {
        assert!(StaticAuthorizer::parse("read-energy *").is_err());
        assert!(StaticAuthorizer::parse("fly = *").is_err());
        assert!(StaticAuthorizer::parse("read-energy = user:1").is_err());
        assert!(StaticAuthorizer::parse("read-energy = uid:abc").is_err());
    }
}
