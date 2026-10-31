//! Native Android entrypoint for the existing PhotoCraft egui application.
//!
//! This crate is intentionally a thin platform host. The editor, command system, document model,
//! and renderer continue to come from the existing Rust workspace crates.

#[cfg(target_os = "android")]
mod entrypoint;
#[cfg(target_os = "android")]
mod services;
#[cfg(target_os = "android")]
mod saf;
