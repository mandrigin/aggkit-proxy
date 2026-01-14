//! Miden RPC Proxy
//!
//! JSON-RPC proxy server bridging Ethereum-style RPC to Miden network.

#![deny(missing_docs)]
#![deny(clippy::all)]

pub mod client;
pub mod config;
pub mod error;
pub mod receipt;
pub mod types;

pub use config::{ConfigError, ProxyConfig};
pub use error::{ProxyError, ProxyResult};
pub use receipt::{
    miden_tx_to_eth_receipt, poll_tx_confirmation, EthReceipt, EthTxHash, MidenTxId,
    TxHashMap, TxStatus, BRIDGE_CONTRACT_ADDRESS,
};
pub use types::ClaimAssetParams;
