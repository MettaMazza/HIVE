#![allow(clippy::redundant_field_names, clippy::collapsible_if)]
pub mod drives;
pub mod goals;
pub mod inbox;
pub mod outreach;
pub mod telemetry;
pub mod react;
pub mod repair;
pub mod builder;
pub mod core;

pub use builder::EngineBuilder;

#[cfg(test)]
pub mod tests;
