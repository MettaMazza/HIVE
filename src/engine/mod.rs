#![allow(clippy::redundant_field_names, clippy::collapsible_if)]
pub mod drives;
pub mod goals;
pub mod goal_store;
pub mod inbox;
pub mod outreach;
pub mod telemetry;
pub mod react;
pub mod repair;
pub mod builder;
pub mod core;
pub(crate) mod core_pipeline;
pub mod email_watcher;
pub mod chronos;
pub mod checkpoint;
pub mod consolidator;

pub use builder::EngineBuilder;

#[cfg(test)]
pub mod tests;
#[cfg(test)]
pub mod tests_commands;
#[cfg(test)]
pub mod tests_telemetry;
