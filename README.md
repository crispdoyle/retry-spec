# retry-spec

Every service ends up with retry logic, and it ends up as scattered
constants: a `MAX_RETRIES = 5` here, a hand-rolled exponential backoff loop
there, jitter added or not depending on who wrote the code last. There is
no shared way to write down "retry up to 5 times, exponential backoff
starting at 100ms doubling up to 30s, full jitter" that can be put in a
config file, diffed in a pull request, or checked before it reaches
production.

This crate defines a small text format for that, and two pure functions
around it: `parse`, which turns text into a validated `RetryPolicy` or a
specific error, and `format`, which turns a `RetryPolicy` back into the
canonical text. Neither touches a clock, a file, or the network, so both
are trivial to unit test with plain string comparisons.

## Example

```
retry max_attempts=5 backoff=exponential(base=100ms, factor=2.0, max=30s) jitter=full retry_on=timeout,unavailable
```

```rust
use retry_spec::{parse, format, Backoff};

let policy = parse(
    "retry max_attempts=5 backoff=exponential(base=100ms, factor=2.0, max=30s) jitter=full"
).expect("valid policy");

assert_eq!(policy.max_attempts, 5);
match policy.backoff {
    Backoff::Exponential { factor_thousandths, .. } => assert_eq!(factor_thousandths, 2000),
    _ => unreachable!(),
}

// format() always emits the same canonical layout, so this round-trips.
let text = format(&policy);
assert_eq!(parse(&text).unwrap(), policy);
```

A malformed or nonsensical policy comes back as an error that says where
and why it failed:

```rust
use retry_spec::{parse, ParseError};

let err = parse("retry max_attempts=0 backoff=fixed(delay=1s)").unwrap_err();
assert!(matches!(err, ParseError::Validation { .. }));
```

## Grammar

```
retry <attribute>*

attribute   := max_attempts | backoff | jitter | retry_on
max_attempts := "max_attempts=" <uint>            -- required, 1..=1000
backoff     := "backoff=" fixed | linear | exponential  -- required
fixed       := "fixed(delay=" <duration> ")"
linear      := "linear(base=" <duration> ", step=" <duration> ")"
exponential := "exponential(base=" <duration> ", factor=" <decimal>
               [", max=" <duration>] ")"
jitter      := "jitter=" "none" | "full" | "equal"  -- optional, default none
retry_on    := "retry_on=" <ident> ("," <ident>)*   -- optional, default empty

duration := <uint> ("ms" | "s" | "m" | "h")
decimal  := <uint> ["." <digit>{1,3}]
```

Attributes may appear in any order and are separated by whitespace.
`factor` must be greater than 1.0, and `max` (if given) must be at least
`base` — an exponential backoff that never grows or that is capped below
its own starting point is rejected by `validate` rather than silently
accepted.

## Design notes

- No third-party dependencies, standard library only.
- The exponential growth factor is stored internally as thousandths
  (`2.0` becomes `2000`) instead of an `f64`, so `RetryPolicy` can derive
  `Eq` and so `format` never has to worry about float rounding producing
  a different string than the one that was parsed.
- `validate` is exposed on its own, separate from `parse`, because
  `RetryPolicy`'s fields are public — code that builds one directly
  (rather than through `parse`) can still run the same checks.

## License

MIT, see `LICENSE`.
