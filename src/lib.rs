//! Miden RPC Proxy
//!
//! JSON-RPC proxy server bridging Ethereum-style RPC to Miden network.

#![deny(missing_docs)]
#![deny(clippy::all)]

pub mod address_mapper;
pub mod claim_tracker;
pub mod client;
pub mod config;
pub mod decode;
pub mod error;
pub mod receipt;
pub mod storage;
pub mod types;

// Re-exports for convenience
pub use address_mapper::{AddressMapper, AddressMapperConfig, EthAddress, MidenAccountId};
pub use claim_tracker::ClaimTracker;
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
pub use storage::{AddressMapping, MappingStorage};
pub use types::ClaimAssetParams;

// Re-export client module types
// NOTE: miden-agglayer helpers (create_claim_note, evm_address_to_account_id) are
// not available due to version incompatibility with miden-client 0.12.
// When versions align, we can use miden-agglayer's create_claim_note() instead.
pub use client::{
    build_claim_transaction_request, create_bridge_claim_note, init_client, submit_transaction,
    sync_state, BridgeClaimParams, ClientError, MidenClientConfig, MidenClientWrapper, SyncSummary,
};
