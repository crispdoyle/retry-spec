use crate::model::{Backoff, Jitter, RetryPolicy};
use std::time::Duration;

/// Renders a policy back into the text format `parse` accepts. Always
/// produces the same canonical layout (fixed key order, jitter spelled
/// out even when it is `none`), so two policies that are equal also
/// format identically, and `parse(format(p)) == p` for any `p` that
/// already satisfies `validate`.
pub fn format(policy: &RetryPolicy) -> String {
    let mut out = String::from("retry");
    out.push_str(&format!(" max_attempts={}", policy.max_attempts));
    out.push_str(&format!(" backoff={}", format_backoff(&policy.backoff)));
    out.push_str(&format!(" jitter={}", format_jitter(policy.jitter)));
    if !policy.retry_on.is_empty() {
        out.push_str(" retry_on=");
        out.push_str(&policy.retry_on.join(","));
    }
    out
}

fn format_backoff(backoff: &Backoff) -> String {
    match backoff {
        Backoff::Fixed { delay } => format!("fixed(delay={})", format_duration(*delay)),
        Backoff::Linear { base, step } => format!(
            "linear(base={}, step={})",
            format_duration(*base),
            format_duration(*step)
        ),
        Backoff::Exponential {
            base,
            factor_thousandths,
            max,
        } => {
            let mut s = format!(
                "exponential(base={}, factor={}",
                format_duration(*base),
                format_factor(*factor_thousandths)
            );
            if let Some(max) = max {
                s.push_str(&format!(", max={}", format_duration(*max)));
            }
            s.push(')');
            s
        }
    }
}

fn format_jitter(jitter: Jitter) -> &'static str {
    match jitter {
        Jitter::None => "none",
        Jitter::Full => "full",
        Jitter::Equal => "equal",
    }
}

/// Picks the largest unit (h, m, s, ms) that represents the duration with
/// no remainder, so round numbers stay readable instead of always coming
/// out in milliseconds.
fn format_duration(d: Duration) -> String {
    let total_ms = d.as_millis();
    if total_ms == 0 {
        return "0ms".to_string();
    }
    if total_ms % 3_600_000 == 0 {
        format!("{}h", total_ms / 3_600_000)
    } else if total_ms % 60_000 == 0 {
        format!("{}m", total_ms / 60_000)
    } else if total_ms % 1_000 == 0 {
        format!("{}s", total_ms / 1_000)
    } else {
        format!("{total_ms}ms")
    }
}

fn format_factor(thousandths: u32) -> String {
    let whole = thousandths / 1000;
    let frac = thousandths % 1000;
    if frac == 0 {
        return format!("{whole}.0");
    }
    let mut frac_str = format!("{frac:03}");
    while frac_str.len() > 1 && frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{whole}.{frac_str}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_picks_the_largest_clean_unit() {
        assert_eq!(format_duration(Duration::from_millis(250)), "250ms");
        assert_eq!(format_duration(Duration::from_secs(90)), "90s");
        assert_eq!(format_duration(Duration::from_secs(120)), "2m");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h");
    }

    #[test]
    fn factor_trims_trailing_zeros_but_keeps_one_decimal() {
        assert_eq!(format_factor(2000), "2.0");
        assert_eq!(format_factor(1500), "1.5");
        assert_eq!(format_factor(1234), "1.234");
        assert_eq!(format_factor(1230), "1.23");
    }

    #[test]
    fn formats_a_full_policy_in_canonical_order() {
        let policy = RetryPolicy {
            max_attempts: 3,
            backoff: Backoff::Fixed {
                delay: Duration::from_millis(200),
            },
            jitter: Jitter::None,
            retry_on: Vec::new(),
        };
        assert_eq!(
            format(&policy),
            "retry max_attempts=3 backoff=fixed(delay=200ms) jitter=none"
        );
    }
}
