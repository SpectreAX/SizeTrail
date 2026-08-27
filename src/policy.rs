#![allow(clippy::disallowed_types)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProbeId(&'static str);

impl ProbeId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProbePolicy {
    pub id: ProbeId,
    pub max_calls_per_scan: usize,
    pub disable_env: &'static str,
}

pub const SIDE_EFFECT_REGISTRY: &[ProbePolicy] = &[];

#[derive(Debug, Eq, PartialEq)]
pub enum PolicyError {
    UndeclaredProbe(ProbeId),
    Disabled(ProbeId),
    CallLimitExceeded(ProbeId),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (message, id) = match self {
            Self::UndeclaredProbe(id) => ("undeclared probe", id),
            Self::Disabled(id) => ("probe disabled by environment", id),
            Self::CallLimitExceeded(id) => ("probe call limit exceeded", id),
        };
        write!(formatter, "{message}: {}", id.as_str())
    }
}

impl Error for PolicyError {}

pub struct InvocationTracker<'a> {
    policies: &'a [ProbePolicy],
    counts: BTreeMap<ProbeId, usize>,
}

impl InvocationTracker<'static> {
    pub fn for_scan() -> Self {
        Self {
            policies: SIDE_EFFECT_REGISTRY,
            counts: BTreeMap::new(),
        }
    }
}

impl<'a> InvocationTracker<'a> {
    #[cfg(test)]
    fn for_test(policies: &'a [ProbePolicy]) -> Self {
        Self {
            policies,
            counts: BTreeMap::new(),
        }
    }

    pub fn invoke<T>(
        &mut self,
        id: ProbeId,
        invocation: impl FnOnce() -> T,
    ) -> Result<T, PolicyError> {
        let policy = self
            .policies
            .iter()
            .find(|policy| policy.id == id)
            .ok_or(PolicyError::UndeclaredProbe(id))?;

        if std::env::var_os(policy.disable_env).is_some() {
            return Err(PolicyError::Disabled(id));
        }

        let count = self.counts.entry(id).or_default();
        if *count >= policy.max_calls_per_scan {
            return Err(PolicyError::CallLimitExceeded(id));
        }

        *count += 1;
        Ok(invocation())
    }

    pub fn count(&self, id: ProbeId) -> usize {
        self.counts.get(&id).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{InvocationTracker, PolicyError, ProbeId, ProbePolicy};

    #[test]
    fn production_tracker_uses_the_compiled_registry() {
        const UNKNOWN_ID: ProbeId = ProbeId::new("fixture.unknown");
        let mut tracker = InvocationTracker::for_scan();

        assert_eq!(
            tracker.invoke(UNKNOWN_ID, || ()),
            Err(PolicyError::UndeclaredProbe(UNKNOWN_ID))
        );
    }

    #[test]
    fn registry_caps_declared_probes_and_rejects_undeclared_probes_before_invocation() {
        const DECLARED_ID: ProbeId = ProbeId::new("fixture.read_only");
        const UNDECLARED_ID: ProbeId = ProbeId::new("fixture.undeclared");
        const POLICIES: &[ProbePolicy] = &[ProbePolicy {
            id: DECLARED_ID,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_FIXTURE_PROBE",
        }];

        let mut tracker = InvocationTracker::for_test(POLICIES);
        let mut actual_calls = 0;

        tracker
            .invoke(DECLARED_ID, || actual_calls += 1)
            .expect("the declared call is within its limit");
        assert!(tracker.invoke(DECLARED_ID, || actual_calls += 1).is_err());
        assert!(tracker.invoke(UNDECLARED_ID, || actual_calls += 1).is_err());

        assert_eq!(actual_calls, 1);
        assert_eq!(tracker.count(DECLARED_ID), 1);
        assert_eq!(tracker.count(UNDECLARED_ID), 0);
    }
}
