//! OANDA infrastructure - Forex sector provider.
//!
//! Provides [OandaSectorProvider]. Market data and execution for OANDA v20 API
//! can be added in future via dedicated modules.

pub mod client;

pub use client::OandaSectorProvider;
