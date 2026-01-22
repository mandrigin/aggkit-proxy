//! Miden RPC Proxy
//!
//! JSON-RPC proxy server bridging Ethereum-style RPC to Miden network.

#![deny(missing_docs)]
#![deny(clippy::all)]

pub mod address_mapper;
pub mod agglayer_faucet;
pub mod block_state;
pub mod claim_tracker;
pub mod client;
pub mod config;
pub mod decode;
pub mod error;
pub mod log_synthesis;
pub mod receipt;
pub mod storage;
pub mod types;

// Re-exports for convenience
pub use address_mapper::{AddressMapper, AddressMapperConfig, EthAddress, MidenAccountId};
pub use agglayer_faucet::{create_and_deploy_agglayer_faucet, AgglayerFaucetResult};
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

// Re-export block state and log synthesis for kurtosis-cdk integration
pub use block_state::{BlockState, SyntheticBlock};
pub use log_synthesis::{LogFilter, LogStore, SyntheticLog, BRIDGE_EVENT_TOPIC, CLAIM_EVENT_TOPIC};

// Re-export client module types
// Uses miden-agglayer's create_claim_note() for CLAIM note creation with SMT proof validation.
pub use client::{
    build_claim_transaction_request, create_bridge_claim_note, init_client, submit_transaction,
    sync_state, BridgeClaimParams, ClientError, MidenClientConfig, MidenClientWrapper, SyncSummary,
};
