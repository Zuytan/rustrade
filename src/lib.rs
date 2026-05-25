pub mod application;
pub mod config;
pub mod domain;
pub mod infrastructure;
#[cfg(feature = "ui")]
pub mod interfaces;

#[cfg(test)]
mod config_tests;
