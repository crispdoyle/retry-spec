//! A small text format for retry policies, plus a validating parser and a
//! pretty printer for it. See the README for the grammar and examples.

mod model;
mod parser;
mod printer;

pub use model::{Backoff, Jitter, RetryPolicy};
pub use parser::{parse, validate, ParseError};
pub use printer::format;
