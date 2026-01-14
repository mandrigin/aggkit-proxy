//! Miden RPC Proxy
//!
//! JSON-RPC proxy server bridging Ethereum-style RPC to Miden network.

#![deny(missing_docs)]
#![deny(clippy::all)]

pub mod claim_tracker;
pub mod config;
pub mod error;
pub mod types;

pub use claim_tracker::ClaimTracker;
pub use config::{ConfigError, ProxyConfig};
pub use error::{ProxyError, ProxyResult};
pub use types::ClaimAssetParams;
