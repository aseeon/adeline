//! Pins the shape of every message between the engine and its clients.
//! `tests/wire/<PROTOCOL>.jsonl` holds a sample of each message, and each must
//! read back unchanged, so a renamed, added, removed or retyped field fails here.
//!
//! When this fails, the change breaks engines and clients on other machines:
//! bump `PROTOCOL` in `protocol.rs`, rename the samples file to the new number,
//! and replace the failing lines with what the failure says they now read as.
//! Collections hold one entry, so a sample pins each element's shape too.

use adeline_core::protocol::{ClientMessage, Command, Delta, EngineMessage, PROTOCOL};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn samples() -> Vec<Value> {
    let path = format!("{}/tests/wire/{PROTOCOL}.jsonl", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{path}: {error}. A PROTOCOL bump renames the samples file to the new number.")
    });
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn read_back(sample: &Value) -> Result<Value, serde_json::Error> {
    if matches!(sample["type"].as_str(), Some("hello" | "request")) {
        serde_json::to_value(serde_json::from_value::<ClientMessage>(sample.clone())?)
    } else {
        serde_json::to_value(serde_json::from_value::<EngineMessage>(sample.clone())?)
    }
}

// serde lists every variant when it meets an unknown one.
fn variants<T: DeserializeOwned + std::fmt::Debug>(tag: &str) -> Vec<String> {
    let error = serde_json::from_value::<T>(json!({ tag: "?" }))
        .unwrap_err()
        .to_string();
    // "unknown variant `?`, expected `a` or `b`" or "…expected one of `a`, `b`, `c`"
    error
        .split('`')
        .skip(3)
        .step_by(2)
        .map(Into::into)
        .collect()
}

#[test]
fn every_message_reads_back_unchanged() {
    let changed: Vec<String> = samples()
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| match read_back(sample) {
            Ok(again) if again == *sample => None,
            Ok(again) => Some(format!("line {} now reads as\n{again}", index + 1)),
            Err(error) => Some(format!("line {}: {error}", index + 1)),
        })
        .collect();
    assert!(
        changed.is_empty(),
        "The wire format changed. Bump PROTOCOL (see this file's header).\n\n{}",
        changed.join("\n\n")
    );
}

#[test]
fn every_message_kind_has_a_sample() {
    let samples = samples();
    let has =
        |at: &dyn Fn(&Value) -> &Value, name: &str| samples.iter().any(|sample| at(sample) == name);
    let mut missing = Vec::new();
    for name in variants::<ClientMessage>("type")
        .into_iter()
        .chain(variants::<EngineMessage>("type"))
    {
        if !has(&|sample| &sample["type"], &name) {
            missing.push(format!("type {name}"));
        }
    }
    for name in variants::<Command>("op") {
        if !has(&|sample| &sample["command"]["op"], &name) {
            missing.push(format!("command {name}"));
        }
    }
    for name in variants::<Delta>("delta") {
        if !has(&|sample| &sample["delta"], &name) {
            missing.push(format!("delta {name}"));
        }
    }
    assert!(
        missing.is_empty(),
        "tests/wire/{PROTOCOL}.jsonl has no sample of: {}",
        missing.join(", ")
    );
}
