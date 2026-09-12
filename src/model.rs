use std::time::Duration;

/// A validated retry policy. The only way to obtain one from untrusted
/// input is [`crate::parse`], which runs [`crate::validate`] before
/// returning. Building one by hand (all fields are public, for tests and
/// for callers who already know their values are sound) skips that check,
/// so run it yourself if the values did not come from `parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: Backoff,
    pub jitter: Jitter,
    /// Error codes this policy applies to. An empty list means "retry on
    /// anything the caller considers retryable".
    pub retry_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backoff {
    Fixed {
        delay: Duration,
    },
    Linear {
        base: Duration,
        step: Duration,
    },
    Exponential {
        base: Duration,
        /// The growth factor, scaled by 1000 (so 2.5 is stored as 2500).
        /// Fixed-point storage keeps this type `Eq` and keeps the pretty
        /// printer's output deterministic, which a plain `f64` would not.
        factor_thousandths: u32,
        max: Option<Duration>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jitter {
    None,
    Full,
    Equal,
}
