//! CLI for checking and reformatting retry policy files.
//!
//! Usage:
//!   retry-spec check <file>          exit 0 if the file holds a valid policy, 1 otherwise
//!   retry-spec fmt <file>            print the canonical form to stdout
//!   retry-spec fmt --write <file>    rewrite the file in place with the canonical form

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [command, path] if command == "check" => check(path),
        [command, path] if command == "fmt" => fmt(path, false),
        [command, flag, path] if command == "fmt" && flag == "--write" => fmt(path, true),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage:\n  retry-spec check <file>\n  retry-spec fmt [--write] <file>".to_string()
}

fn read(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{path}: {err}"))
}

fn check(path: &str) -> Result<(), String> {
    let text = read(path)?;
    retry_spec::parse(&text).map_err(|err| format!("{path}: {err}"))?;
    println!("{path}: ok");
    Ok(())
}

fn fmt(path: &str, write: bool) -> Result<(), String> {
    let text = read(path)?;
    let policy = retry_spec::parse(&text).map_err(|err| format!("{path}: {err}"))?;
    let canonical = retry_spec::format(&policy);
    if write {
        fs::write(path, format!("{canonical}\n")).map_err(|err| format!("{path}: {err}"))?;
    } else {
        println!("{canonical}");
    }
    Ok(())
}
