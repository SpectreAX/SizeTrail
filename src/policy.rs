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

impl<'a> InvocationTracker<'a> {
    pub fn new(policies: &'a [ProbePolicy]) -> Self {
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
