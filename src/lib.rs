//! Miden Ethereum Proxy - Bridge operations via JSON-RPC

pub mod receipt;

pub use receipt::{
    miden_tx_to_eth_receipt, poll_tx_confirmation, EthReceipt, EthTxHash, MidenTxId,
    TxHashMap, TxStatus, BRIDGE_CONTRACT_ADDRESS,
};
