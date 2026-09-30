use crate::model::{Backoff, Jitter, RetryPolicy};
use std::fmt;
use std::time::Duration;

/// Why a policy string was rejected. `Syntax` covers malformed text
/// (wrong keyword, unknown key, bad number); `Validation` covers text
/// that parses fine but describes a policy that cannot make sense, such
/// as zero attempts or a shrinking exponential backoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Syntax { position: usize, message: String },
    Validation { message: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Syntax { position, message } => {
                write!(f, "syntax error at position {position}: {message}")
            }
            ParseError::Validation { message } => write!(f, "validation error: {message}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses and validates a retry policy string, e.g.
/// `retry max_attempts=5 backoff=exponential(base=100ms, factor=2.0, max=30s) jitter=full`.
/// See the crate README for the full grammar.
pub fn parse(input: &str) -> Result<RetryPolicy, ParseError> {
    let mut cursor = Cursor::new(input);
    cursor.skip_whitespace();
    expect_keyword(&mut cursor, "retry")?;

    let mut max_attempts: Option<u32> = None;
    let mut backoff: Option<Backoff> = None;
    let mut jitter: Option<Jitter> = None;
    let mut retry_on: Option<Vec<String>> = None;

    loop {
        cursor.skip_whitespace();
        if cursor.at_end() {
            break;
        }

        let key_pos = cursor.pos;
        let key = parse_ident(&mut cursor)?;
        cursor.skip_whitespace();
        expect_char(&mut cursor, '=')?;
        cursor.skip_whitespace();

        match key.as_str() {
            "max_attempts" => {
                if max_attempts.is_some() {
                    return Err(cursor.error_at(key_pos, "duplicate key 'max_attempts'"));
                }
                let value = parse_uint(&mut cursor)?;
                let value: u32 = value
                    .try_into()
                    .map_err(|_| cursor.error("max_attempts is too large"))?;
                max_attempts = Some(value);
            }
            "backoff" => {
                if backoff.is_some() {
                    return Err(cursor.error_at(key_pos, "duplicate key 'backoff'"));
                }
                backoff = Some(parse_backoff(&mut cursor)?);
            }
            "jitter" => {
                if jitter.is_some() {
                    return Err(cursor.error_at(key_pos, "duplicate key 'jitter'"));
                }
                jitter = Some(parse_jitter(&mut cursor)?);
            }
            "retry_on" => {
                if retry_on.is_some() {
                    return Err(cursor.error_at(key_pos, "duplicate key 'retry_on'"));
                }
                retry_on = Some(parse_retry_on(&mut cursor)?);
            }
            other => {
                return Err(cursor.error_at(key_pos, format!("unknown key '{other}'")));
            }
        }
    }

    let max_attempts = max_attempts.ok_or_else(|| ParseError::Validation {
        message: "missing required key 'max_attempts'".to_string(),
    })?;
    let backoff = backoff.ok_or_else(|| ParseError::Validation {
        message: "missing required key 'backoff'".to_string(),
    })?;

    let policy = RetryPolicy {
        max_attempts,
        backoff,
        jitter: jitter.unwrap_or(Jitter::None),
        retry_on: retry_on.unwrap_or_default(),
    };
    validate(&policy)?;
    Ok(policy)
}

/// Checks the semantic rules a syntactically well-formed policy still has
/// to satisfy: at least one attempt, positive delays, a growth factor
/// greater than 1.0, and a cap no smaller than the base delay. Exposed
/// separately from `parse` so a `RetryPolicy` built by hand can be checked
/// too.
pub fn validate(policy: &RetryPolicy) -> Result<(), ParseError> {
    if policy.max_attempts == 0 {
        return Err(ParseError::Validation {
            message: "max_attempts must be at least 1".to_string(),
        });
    }
    if policy.max_attempts > 1_000 {
        return Err(ParseError::Validation {
            message: "max_attempts must be at most 1000".to_string(),
        });
    }

    match &policy.backoff {
        Backoff::Fixed { delay } => {
            if delay.is_zero() {
                return Err(ParseError::Validation {
                    message: "fixed backoff delay must be greater than zero".to_string(),
                });
            }
        }
        Backoff::Linear { base, step } => {
            if base.is_zero() {
                return Err(ParseError::Validation {
                    message: "linear backoff base must be greater than zero".to_string(),
                });
            }
            if step.is_zero() {
                return Err(ParseError::Validation {
                    message: "linear backoff step must be greater than zero".to_string(),
                });
            }
        }
        Backoff::Exponential {
            base,
            factor_thousandths,
            max,
        } => {
            if base.is_zero() {
                return Err(ParseError::Validation {
                    message: "exponential backoff base must be greater than zero".to_string(),
                });
            }
            if *factor_thousandths <= 1_000 {
                return Err(ParseError::Validation {
                    message: "exponential backoff factor must be greater than 1.0".to_string(),
                });
            }
            if let Some(max) = max {
                if max < base {
                    return Err(ParseError::Validation {
                        message: "exponential backoff max must be at least base".to_string(),
                    });
                }
            }
        }
    }

    for name in &policy.retry_on {
        if name.is_empty() {
            return Err(ParseError::Validation {
                message: "retry_on entries must not be empty".to_string(),
            });
        }
    }

    Ok(())
}

enum ArgValue {
    Duration(Duration),
    Decimal(u32),
}

fn parse_backoff(cursor: &mut Cursor) -> Result<Backoff, ParseError> {
    let name_pos = cursor.pos;
    let name = parse_ident(cursor)?;
    cursor.skip_whitespace();
    expect_char(cursor, '(')?;
    cursor.skip_whitespace();

    let mut args: Vec<(String, ArgValue)> = Vec::new();
    if cursor.peek() == Some(')') {
        cursor.bump();
    } else {
        let allowed: &[&str] = match name.as_str() {
            "fixed" => &["delay"],
            "linear" => &["base", "step"],
            "exponential" => &["base", "factor", "max"],
            _ => &[],
        };
        let is_known_kind = !allowed.is_empty();

        loop {
            let arg_key_pos = cursor.pos;
            let arg_key = parse_ident(cursor)?;

            if is_known_kind && !allowed.contains(&arg_key.as_str()) {
                return Err(cursor.error_at(
                    arg_key_pos,
                    format!("unknown argument '{arg_key}' for backoff kind '{name}'"),
                ));
            }
            if args.iter().any(|(k, _)| k == &arg_key) {
                return Err(cursor.error_at(arg_key_pos, format!("duplicate argument '{arg_key}'")));
            }

            cursor.skip_whitespace();
            expect_char(cursor, '=')?;
            cursor.skip_whitespace();

            let value = if arg_key == "factor" {
                ArgValue::Decimal(parse_decimal_thousandths(cursor)?)
            } else {
                ArgValue::Duration(parse_duration(cursor)?)
            };
            args.push((arg_key, value));

            cursor.skip_whitespace();
            match cursor.bump() {
                Some(',') => {
                    cursor.skip_whitespace();
                    continue;
                }
                Some(')') => break,
                _ => return Err(cursor.error("expected ',' or ')' in backoff arguments")),
            }
        }
    }

    build_backoff(name_pos, &name, args)
}

fn build_backoff(name_pos: usize, name: &str, args: Vec<(String, ArgValue)>) -> Result<Backoff, ParseError> {
    fn find_duration(key: &str, args: &[(String, ArgValue)]) -> Option<Duration> {
        args.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
            ArgValue::Duration(d) => Some(*d),
            _ => None,
        })
    }
    fn find_decimal(key: &str, args: &[(String, ArgValue)]) -> Option<u32> {
        args.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
            ArgValue::Decimal(d) => Some(*d),
            _ => None,
        })
    }
    fn required_duration(key: &str, kind: &str, args: &[(String, ArgValue)]) -> Result<Duration, ParseError> {
        find_duration(key, args).ok_or_else(|| ParseError::Validation {
            message: format!("{kind} backoff requires a '{key}' argument"),
        })
    }

    match name {
        "fixed" => Ok(Backoff::Fixed {
            delay: required_duration("delay", "fixed", &args)?,
        }),
        "linear" => Ok(Backoff::Linear {
            base: required_duration("base", "linear", &args)?,
            step: required_duration("step", "linear", &args)?,
        }),
        "exponential" => {
            let base = required_duration("base", "exponential", &args)?;
            let factor_thousandths = find_decimal("factor", &args).ok_or_else(|| ParseError::Validation {
                message: "exponential backoff requires a 'factor' argument".to_string(),
            })?;
            let max = find_duration("max", &args);
            Ok(Backoff::Exponential {
                base,
                factor_thousandths,
                max,
            })
        }
        other => Err(ParseError::Syntax {
            position: name_pos,
            message: format!("unknown backoff kind '{other}', expected fixed, linear, or exponential"),
        }),
    }
}

fn parse_jitter(cursor: &mut Cursor) -> Result<Jitter, ParseError> {
    let pos = cursor.pos;
    let ident = parse_ident(cursor)?;
    match ident.as_str() {
        "none" => Ok(Jitter::None),
        "full" => Ok(Jitter::Full),
        "equal" => Ok(Jitter::Equal),
        other => Err(ParseError::Syntax {
            position: pos,
            message: format!("unknown jitter kind '{other}', expected none, full, or equal"),
        }),
    }
}

fn parse_retry_on(cursor: &mut Cursor) -> Result<Vec<String>, ParseError> {
    let mut names = Vec::new();
    loop {
        let pos = cursor.pos;
        let name = parse_ident(cursor)?;
        if names.contains(&name) {
            return Err(cursor.error_at(pos, format!("duplicate error code '{name}' in retry_on")));
        }
        names.push(name);

        cursor.skip_whitespace();
        if cursor.peek() == Some(',') {
            cursor.bump();
            cursor.skip_whitespace();
            continue;
        }
        break;
    }
    Ok(names)
}

fn parse_duration(cursor: &mut Cursor) -> Result<Duration, ParseError> {
    let value = parse_uint(cursor)?;
    let unit_start = cursor.pos;
    let mut unit = String::new();
    while matches!(cursor.peek(), Some(c) if c.is_alphabetic()) {
        unit.push(cursor.bump().unwrap());
    }
    match unit.as_str() {
        "ms" => Ok(Duration::from_millis(value)),
        "s" => Ok(Duration::from_secs(value)),
        "m" => Ok(Duration::from_secs(value.saturating_mul(60))),
        "h" => Ok(Duration::from_secs(value.saturating_mul(3600))),
        other => Err(cursor.error_at(
            unit_start,
            format!("unknown duration unit '{other}', expected one of ms, s, m, h"),
        )),
    }
}

/// Parses a decimal like `2` or `2.5` into thousandths (`2000`, `2500`).
/// Fixed-point instead of `f64` so the value round-trips exactly through
/// the pretty printer.
fn parse_decimal_thousandths(cursor: &mut Cursor) -> Result<u32, ParseError> {
    let start = cursor.pos;
    let whole = parse_uint(cursor)?;
    let mut frac: u32 = 0;
    if cursor.peek() == Some('.') {
        cursor.bump();
        let mut digits = String::new();
        while matches!(cursor.peek(), Some(c) if c.is_ascii_digit()) {
            digits.push(cursor.bump().unwrap());
        }
        if digits.is_empty() {
            return Err(cursor.error("expected digits after decimal point"));
        }
        if digits.len() > 3 {
            return Err(cursor.error_at(start, "at most 3 decimal places are supported"));
        }
        while digits.len() < 3 {
            digits.push('0');
        }
        frac = digits.parse().unwrap();
    }
    let whole: u32 = whole
        .try_into()
        .map_err(|_| cursor.error_at(start, "number is too large"))?;
    Ok(whole.saturating_mul(1000).saturating_add(frac))
}

fn parse_uint(cursor: &mut Cursor) -> Result<u64, ParseError> {
    let start = cursor.pos;
    let mut digits = String::new();
    while matches!(cursor.peek(), Some(c) if c.is_ascii_digit()) {
        digits.push(cursor.bump().unwrap());
    }
    if digits.is_empty() {
        return Err(cursor.error_at(start, "expected a number"));
    }
    digits
        .parse::<u64>()
        .map_err(|_| cursor.error_at(start, "number is too large"))
}

fn parse_ident(cursor: &mut Cursor) -> Result<String, ParseError> {
    match cursor.peek() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return Err(cursor.error("expected an identifier")),
    }
    let mut ident = String::new();
    while matches!(cursor.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
        ident.push(cursor.bump().unwrap());
    }
    Ok(ident)
}

fn expect_keyword(cursor: &mut Cursor, keyword: &str) -> Result<(), ParseError> {
    let start = cursor.pos;
    for expected in keyword.chars() {
        match cursor.bump() {
            Some(c) if c == expected => {}
            _ => return Err(cursor.error_at(start, format!("expected '{keyword}'"))),
        }
    }
    Ok(())
}

fn expect_char(cursor: &mut Cursor, expected: char) -> Result<(), ParseError> {
    let pos = cursor.pos;
    match cursor.bump() {
        Some(c) if c == expected => Ok(()),
        _ => Err(cursor.error_at(pos, format!("expected '{expected}'"))),
    }
}

/// A position tracked over the input's characters (not bytes), so error
/// positions stay simple even though this format only needs ASCII.
struct Cursor {
    chars: Vec<char>,
    pos: usize,
}

impl Cursor {
    fn new(input: &str) -> Self {
        Cursor {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::Syntax {
            position: self.pos,
            message: message.into(),
        }
    }

    fn error_at(&self, position: usize, message: impl Into<String>) -> ParseError {
        ParseError::Syntax {
            position,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format;

    #[test]
    fn parses_a_full_policy() {
        let input = "retry max_attempts=5 backoff=exponential(base=100ms, factor=2.0, max=30s) jitter=full retry_on=timeout,unavailable";
        let policy = parse(input).unwrap();

        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.jitter, Jitter::Full);
        assert_eq!(
            policy.retry_on,
            vec!["timeout".to_string(), "unavailable".to_string()]
        );
        match policy.backoff {
            Backoff::Exponential {
                base,
                factor_thousandths,
                max,
            } => {
                assert_eq!(base, Duration::from_millis(100));
                assert_eq!(factor_thousandths, 2000);
                assert_eq!(max, Some(Duration::from_secs(30)));
            }
            other => panic!("unexpected backoff: {other:?}"),
        }
    }

    #[test]
    fn format_then_parse_round_trips() {
        let input = "retry max_attempts=3 backoff=fixed(delay=200ms) jitter=none";
        let policy = parse(input).unwrap();
        let reparsed = parse(&format(&policy)).unwrap();
        assert_eq!(policy, reparsed);
    }

    #[test]
    fn rejects_zero_max_attempts() {
        let err = parse("retry max_attempts=0 backoff=fixed(delay=1s)").unwrap_err();
        assert!(matches!(err, ParseError::Validation { .. }));
    }

    #[test]
    fn rejects_a_shrinking_exponential_cap() {
        let err =
            parse("retry max_attempts=3 backoff=exponential(base=1s, factor=2.0, max=500ms)").unwrap_err();
        assert!(matches!(err, ParseError::Validation { .. }));
    }

    #[test]
    fn rejects_unknown_key() {
        let err = parse("retry max_attempts=1 backoff=fixed(delay=1s) bogus=1").unwrap_err();
        assert!(matches!(err, ParseError::Syntax { .. }));
    }
}
