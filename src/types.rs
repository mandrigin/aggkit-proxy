//! Core types for the Miden RPC proxy.

use alloy_primitives::{Address, Bytes, B256, U256};
use serde::{Deserialize, Serialize};

/// Parameters for the claimAsset bridge function.
///
/// This struct represents the decoded parameters from a claimAsset call
/// (selector 0x2cffd02e) on the bridge contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimAssetParams {
    /// SMT proof for the local exit root (array of 32-byte hashes).
    pub smt_proof_local_exit_root: Vec<B256>,

    /// SMT proof for the rollup exit root (array of 32-byte hashes).
    pub smt_proof_rollup_exit_root: Vec<B256>,

    /// Global index encoding claim position.
    ///
    /// Bit layout:
    /// - Bit 64: mainnetFlag (1 = mainnet, 0 = rollup)
    /// - Bits 32-63: rollupIndex (which rollup in the tree)
    /// - Bits 0-31: localRootIndex (position within the local exit tree)
    pub global_index: U256,

    /// Mainnet exit root hash.
    pub mainnet_exit_root: B256,

    /// Rollup exit root hash.
    pub rollup_exit_root: B256,

    /// Origin network ID where the asset was deposited.
    pub origin_network: u32,

    /// Token contract address on the origin network.
    pub origin_token_address: Address,

    /// Destination network ID where the asset is being claimed.
    pub destination_network: u32,

    /// Recipient address on the destination network.
    pub destination_address: Address,

    /// Amount of tokens being claimed.
    pub amount: U256,

    /// Optional metadata associated with the claim.
    pub metadata: Bytes,
}

impl ClaimAssetParams {
    /// Returns whether this claim originated from mainnet.
    ///
    /// Extracts the mainnet flag from bit 64 of the global index.
    pub fn is_mainnet_claim(&self) -> bool {
        (self.global_index >> 64) & U256::from(1) == U256::from(1)
    }

    /// Returns the rollup index from the global index.
    ///
    /// Extracts bits 32-63 from the global index.
    pub fn rollup_index(&self) -> u32 {
        let masked: U256 = (self.global_index >> 32) & U256::from(u32::MAX);
        masked.as_limbs()[0] as u32
    }

    /// Returns the local root index from the global index.
    ///
    /// Extracts bits 0-31 from the global index.
    pub fn local_root_index(&self) -> u32 {
        let masked: U256 = self.global_index & U256::from(u32::MAX);
        masked.as_limbs()[0] as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_index_decoding() {
        let params = ClaimAssetParams {
            smt_proof_local_exit_root: vec![],
            smt_proof_rollup_exit_root: vec![],
            // mainnetFlag=1, rollupIndex=5, localRootIndex=42
            // (1 << 64) | (5 << 32) | 42
            global_index: U256::from(1u128 << 64) | U256::from(5u64 << 32) | U256::from(42u32),
            mainnet_exit_root: B256::ZERO,
            rollup_exit_root: B256::ZERO,
            origin_network: 0,
            origin_token_address: Address::ZERO,
            destination_network: 1,
            destination_address: Address::ZERO,
            amount: U256::from(1000u64),
            metadata: Bytes::new(),
        };

        assert!(params.is_mainnet_claim());
        assert_eq!(params.rollup_index(), 5);
        assert_eq!(params.local_root_index(), 42);
    }

    #[test]
    fn test_rollup_claim() {
        let params = ClaimAssetParams {
            smt_proof_local_exit_root: vec![],
            smt_proof_rollup_exit_root: vec![],
            // mainnetFlag=0, rollupIndex=3, localRootIndex=100
            global_index: U256::from(3u64 << 32) | U256::from(100u32),
            mainnet_exit_root: B256::ZERO,
            rollup_exit_root: B256::ZERO,
            origin_network: 0,
            origin_token_address: Address::ZERO,
            destination_network: 1,
            destination_address: Address::ZERO,
            amount: U256::from(500u64),
            metadata: Bytes::new(),
        };

        assert!(!params.is_mainnet_claim());
        assert_eq!(params.rollup_index(), 3);
        assert_eq!(params.local_root_index(), 100);
    }
}
