//! Error types for the Miden RPC proxy.

use alloy_primitives::U256;
use thiserror::Error;

/// Errors that can occur during proxy operations.
#[derive(Debug, Error)]
pub enum ProxyError {
    /// Failed to decode a raw Ethereum transaction.
    #[error("failed to decode transaction: {0}")]
    TransactionDecode(String),

    /// Transaction is not a claimAsset call.
    #[error("transaction is not a claimAsset call (expected selector 0x2cffd02e, got {selector})")]
    NotClaimAsset {
        /// The actual selector found.
        selector: String,
    },

    /// Failed to decode claimAsset parameters.
    #[error("failed to decode claimAsset parameters: {0}")]
    ClaimDecode(String),

    /// Claim has already been processed (replay attack prevention).
    #[error("claim with global index {global_index} has already been processed")]
    AlreadyClaimed {
        /// The duplicate global index.
        global_index: U256,
    },

    /// Failed to communicate with the Miden node.
    #[error("miden RPC error: {0}")]
    MidenRpc(String),

    /// Failed to create a Miden transaction.
    #[error("failed to create miden transaction: {0}")]
    MidenTransaction(String),

    /// Failed to find or create a Miden account for the recipient.
    #[error("failed to resolve recipient account for {eth_address}: {reason}")]
    AccountResolution {
        /// Ethereum address being resolved.
        eth_address: String,
        /// Reason for the failure.
        reason: String,
    },

    /// Transaction receipt not found (still pending or unknown).
    #[error("transaction receipt not found for {tx_hash}")]
    ReceiptNotFound {
        /// Transaction hash.
        tx_hash: String,
    },

    /// Invalid configuration.
    #[error("configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Result type alias for proxy operations.
pub type ProxyResult<T> = Result<T, ProxyError>;

impl ProxyError {
    /// Returns the JSON-RPC error code for this error.
    ///
    /// Standard JSON-RPC error codes:
    /// - -32700: Parse error
    /// - -32600: Invalid request
    /// - -32601: Method not found
    /// - -32602: Invalid params
    /// - -32603: Internal error
    /// - -32000 to -32099: Server errors (reserved for implementation)
    pub fn rpc_error_code(&self) -> i32 {
        match self {
            ProxyError::TransactionDecode(_) => -32602,
            ProxyError::NotClaimAsset { .. } => -32602,
            ProxyError::ClaimDecode(_) => -32602,
            ProxyError::AlreadyClaimed { .. } => -32000,
            ProxyError::MidenRpc(_) => -32001,
            ProxyError::MidenTransaction(_) => -32002,
            ProxyError::AccountResolution { .. } => -32003,
            ProxyError::ReceiptNotFound { .. } => -32004,
            ProxyError::Config(_) => -32603,
            ProxyError::Internal(_) => -32603,
        }
    }
}
