//! Miden RPC Proxy
//!
//! JSON-RPC proxy server bridging Ethereum-style RPC to Miden network.

#![deny(missing_docs)]
#![deny(clippy::all)]

pub mod client;
pub mod config;
pub mod decode;
pub mod error;
pub mod receipt;
pub mod types;

// Note: These modules are pending merge from other branches:
// pub mod address_mapper;  // mi-w4a (capable)
// pub mod claim_tracker;   // mi-2iy (cheedo)

pub use config::{ConfigError, ProxyConfig};
pub use decode::{
    decode_transaction, is_claim_asset, parse_claim_asset, DecodeError, DecodedTransaction,
    GlobalIndex, CLAIM_ASSET_SELECTOR,
};
pub use error::{ProxyError, ProxyResult};
pub use receipt::{
    miden_tx_to_eth_receipt, poll_tx_confirmation, EthReceipt, EthTxHash, MidenTxId, TxHashMap,
    TxStatus, BRIDGE_CONTRACT_ADDRESS,
};
pub use types::ClaimAssetParams;
