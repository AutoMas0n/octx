//! Library for the octx `agent` arm.
//!
//! The binary entrypoint (`main.rs`) stays thin; all logic lives here so it can
//! be unit- and integration-tested.

pub mod cli;
pub mod events;
pub mod permissions;
pub mod pi;
pub mod runner;
pub mod tool;
