//! Everything the engine and the UI share: the conversation model, the
//! engine ↔ client protocol, agents, settings, machines and the transports
//! that reach an engine. No GPUI.
pub mod agents;
pub mod askpass;
pub mod config;
pub mod conversation;
pub mod data;
pub mod files;
pub mod harness;
pub mod install;
pub mod ipc;
pub mod log;
pub mod machines;
pub mod platform;
pub mod prepared;
pub mod profiles;
pub mod protocol;
pub mod recency;
pub mod remote;
