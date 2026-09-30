//! Round-trip tests over randomly generated policies. There is no random
//! number crate available, so this uses a small xorshift generator with a
//! fixed seed; a failure reproduces exactly and the message prints the
//! policy that broke.

use retry_spec::{format, parse, validate, Backoff, Jitter, RetryPolicy};
use std::time::Duration;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-enough value in `lo..=hi`.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.next() % one_in == 0
    }
}

/// Mixes round values (whole seconds, minutes, hours) with odd millisecond
/// counts so every branch of the printer's unit selection gets used.
fn duration(rng: &mut Rng) -> Duration {
    let millis = match rng.range(0, 3) {
        0 => rng.range(1, 5_000),
        1 => rng.range(1, 600) * 1_000,
        2 => rng.range(1, 120) * 60_000,
        _ => rng.range(1, 48) * 3_600_000,
    };
    Duration::from_millis(millis)
}

fn ident(rng: &mut Rng) -> String {
    const FIRST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_";
    const REST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
    let len = rng.range(1, 12) as usize;
    let mut s = String::new();
    s.push(FIRST[rng.range(0, FIRST.len() as u64 - 1) as usize] as char);
    for _ in 1..len {
        s.push(REST[rng.range(0, REST.len() as u64 - 1) as usize] as char);
    }
    s
}

fn backoff(rng: &mut Rng) -> Backoff {
    match rng.range(0, 2) {
        0 => Backoff::Fixed {
            delay: duration(rng),
        },
        1 => Backoff::Linear {
            base: duration(rng),
            step: duration(rng),
        },
        _ => {
            let base = duration(rng);
            let max = if rng.chance(2) {
                Some(base + duration(rng))
            } else {
                None
            };
            Backoff::Exponential {
                base,
                factor_thousandths: rng.range(1_001, 20_000) as u32,
                max,
            }
        }
    }
}

fn policy(rng: &mut Rng) -> RetryPolicy {
    let mut retry_on: Vec<String> = Vec::new();
    for _ in 0..rng.range(0, 4) {
        let name = ident(rng);
        if !retry_on.contains(&name) {
            retry_on.push(name);
        }
    }
    let jitter = match rng.range(0, 2) {
        0 => Jitter::None,
        1 => Jitter::Full,
        _ => Jitter::Equal,
    };
    RetryPolicy {
        max_attempts: rng.range(1, 1_000) as u32,
        backoff: backoff(rng),
        jitter,
        retry_on,
    }
}

#[test]
fn random_valid_policies_round_trip() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2_000 {
        let original = policy(&mut rng);
        validate(&original).unwrap_or_else(|e| panic!("generator built an invalid policy {original:?}: {e}"));

        let text = format(&original);
        let reparsed = parse(&text).unwrap_or_else(|e| panic!("could not reparse {text:?}: {e}"));
        assert_eq!(reparsed, original, "text was {text:?}");
        assert_eq!(format(&reparsed), text, "format is not stable for {text:?}");
    }
}

#[test]
fn whitespace_and_key_order_do_not_change_the_result() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..500 {
        let original = policy(&mut rng);
        let text = format(&original);

        // Rebuild the text with the attributes reversed and padded. The
        // backoff attribute contains spaces of its own, so split on the
        // key names rather than on whitespace.
        let body = text.strip_prefix("retry ").unwrap();
        let mut starts: Vec<usize> = ["max_attempts=", "backoff=", "jitter=", "retry_on="]
            .iter()
            .filter_map(|key| body.find(key))
            .collect();
        starts.sort_unstable();
        let mut parts: Vec<&str> = Vec::new();
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(body.len());
            parts.push(body[start..end].trim());
        }
        parts.reverse();
        let shuffled = format!("  retry\t{}\n", parts.join("   "));

        let reparsed = parse(&shuffled).unwrap_or_else(|e| panic!("could not parse {shuffled:?}: {e}"));
        assert_eq!(reparsed, original, "text was {shuffled:?}");
    }
}

#[test]
fn truncated_input_never_panics() {
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    for _ in 0..200 {
        let text = format(&policy(&mut rng));
        for end in 0..text.len() {
            // Every truncation is ASCII, so any byte offset is a valid
            // slice boundary. Most are errors; the only requirement is
            // that they come back as errors rather than panics.
            let _ = parse(&text[..end]);
        }
    }
}
