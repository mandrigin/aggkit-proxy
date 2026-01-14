//! Transaction and claimAsset calldata decoding.

use alloy_consensus::TxEnvelope;
use alloy_primitives::{Address, Bytes, TxKind, U256};
use alloy_rlp::Decodable;
use alloy_sol_types::{sol, SolCall};
use thiserror::Error;

/// The claimAsset function selector.
/// Computed from keccak256 of the function signature:
/// claimAsset(bytes32[32],bytes32[32],uint256,bytes32,bytes32,uint32,address,uint32,address,uint256,bytes)
pub const CLAIM_ASSET_SELECTOR: [u8; 4] = [0xcc, 0xaa, 0x2d, 0x11];

/// Errors that can occur during transaction decoding.
#[derive(Debug, Error)]
pub enum DecodeError {
    /// RLP decoding failed.
    #[error("RLP decoding failed: {0}")]
    RlpDecode(String),

    /// Not a claimAsset transaction.
    #[error("Not a claimAsset transaction: selector mismatch")]
    NotClaimAsset,

    /// Invalid calldata format.
    #[error("Invalid calldata: {0}")]
    InvalidCalldata(String),

    /// Transaction has no input data.
    #[error("Transaction has no input data")]
    NoInputData,

    /// Failed to recover transaction signer.
    #[error("Failed to recover signer: {0}")]
    SignerRecovery(String),
}

/// Decoded globalIndex bit fields.
///
/// The globalIndex is a uint256 with the following layout:
/// - Bit 64: mainnetFlag (1 = mainnet, 0 = rollup)
/// - Bits 32-63: rollupIndex
/// - Bits 0-31: localRootIndex (deposit counter within the network)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalIndex {
    /// Whether the origin is mainnet (true) or a rollup (false)
    pub mainnet_flag: bool,
    /// Index of the rollup in the rollup exit tree (only meaningful if mainnet_flag is false)
    pub rollup_index: u32,
    /// Local deposit counter within the origin network
    pub local_root_index: u32,
}

impl GlobalIndex {
    /// Decode a globalIndex from a U256 value.
    ///
    /// Bit layout:
    /// - Bit 64: mainnetFlag
    /// - Bits 32-63: rollupIndex
    /// - Bits 0-31: localRootIndex
    pub fn decode(value: U256) -> Self {
        let limbs = value.as_limbs();
        let low = limbs[0]; // bits 0-63
        let high = limbs[1]; // bits 64-127

        // mainnetFlag is bit 64 (lowest bit of high word)
        let mainnet_flag = (high & 1) != 0;

        // rollupIndex is bits 32-63 (upper 32 bits of low word)
        let rollup_index = (low >> 32) as u32;

        // localRootIndex is bits 0-31 (lower 32 bits of low word)
        let local_root_index = low as u32;

        Self {
            mainnet_flag,
            rollup_index,
            local_root_index,
        }
    }
}

// Define the claimAsset function ABI using alloy's sol! macro.
// Selector: 0x2cffd02e
sol! {
    /// Bridge claimAsset function for claiming bridged assets.
    function claimAsset(
        bytes32[32] smtProofLocalExitRoot,
        bytes32[32] smtProofRollupExitRoot,
        uint256 globalIndex,
        bytes32 mainnetExitRoot,
        bytes32 rollupExitRoot,
        uint32 originNetwork,
        address originTokenAddress,
        uint32 destinationNetwork,
        address destinationAddress,
        uint256 amount,
        bytes metadata
    ) external;
}

/// Parameters extracted from a claimAsset call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimAssetParams {
    /// SMT proof for local exit root (32 siblings)
    pub smt_proof_local_exit_root: [[u8; 32]; 32],
    /// SMT proof for rollup exit root (32 siblings)
    pub smt_proof_rollup_exit_root: [[u8; 32]; 32],
    /// Global index with decoded bit fields
    pub global_index: GlobalIndex,
    /// Raw global index value
    pub global_index_raw: U256,
    /// Mainnet exit root
    pub mainnet_exit_root: [u8; 32],
    /// Rollup exit root
    pub rollup_exit_root: [u8; 32],
    /// Origin network ID
    pub origin_network: u32,
    /// Origin token address on the source chain
    pub origin_token_address: Address,
    /// Destination network ID
    pub destination_network: u32,
    /// Destination address to receive the claim
    pub destination_address: Address,
    /// Amount to claim
    pub amount: U256,
    /// Optional metadata
    pub metadata: Bytes,
}

/// Decoded transaction with extracted input data.
#[derive(Debug)]
pub struct DecodedTransaction {
    /// The destination address (to field)
    pub to: Option<Address>,
    /// The transaction value in wei
    pub value: U256,
    /// The input/calldata
    pub input: Bytes,
    /// The sender address (recovered from signature)
    pub from: Address,
    /// Chain ID (if present)
    pub chain_id: Option<u64>,
}

/// Helper to extract `to` address from TxKind
fn extract_to(kind: TxKind) -> Option<Address> {
    match kind {
        TxKind::Call(addr) => Some(addr),
        TxKind::Create => None,
    }
}

/// Decode a raw RLP-encoded Ethereum transaction.
///
/// Supports legacy, EIP-2930, and EIP-1559 transaction types.
pub fn decode_transaction(raw_tx: &[u8]) -> Result<DecodedTransaction, DecodeError> {
    let tx = TxEnvelope::decode(&mut &raw_tx[..])
        .map_err(|e| DecodeError::RlpDecode(e.to_string()))?;

    // Extract fields based on transaction type
    let (to, value, input, chain_id, from) = match &tx {
        TxEnvelope::Legacy(signed) => {
            let inner = signed.tx();
            let from = signed
                .recover_signer()
                .map_err(|e| DecodeError::SignerRecovery(e.to_string()))?;
            (
                extract_to(inner.to),
                inner.value,
                inner.input.clone(),
                inner.chain_id,
                from,
            )
        }
        TxEnvelope::Eip2930(signed) => {
            let inner = signed.tx();
            let from = signed
                .recover_signer()
                .map_err(|e| DecodeError::SignerRecovery(e.to_string()))?;
            (
                extract_to(inner.to),
                inner.value,
                inner.input.clone(),
                Some(inner.chain_id),
                from,
            )
        }
        TxEnvelope::Eip1559(signed) => {
            let inner = signed.tx();
            let from = signed
                .recover_signer()
                .map_err(|e| DecodeError::SignerRecovery(e.to_string()))?;
            (
                extract_to(inner.to),
                inner.value,
                inner.input.clone(),
                Some(inner.chain_id),
                from,
            )
        }
        TxEnvelope::Eip4844(signed) => {
            let inner = signed.tx().tx();
            let from = signed
                .recover_signer()
                .map_err(|e| DecodeError::SignerRecovery(e.to_string()))?;
            (
                Some(inner.to),
                inner.value,
                inner.input.clone(),
                Some(inner.chain_id),
                from,
            )
        }
        TxEnvelope::Eip7702(signed) => {
            let inner = signed.tx();
            let from = signed
                .recover_signer()
                .map_err(|e| DecodeError::SignerRecovery(e.to_string()))?;
            // EIP-7702 always has a `to` address (no create support)
            (
                Some(inner.to),
                inner.value,
                inner.input.clone(),
                Some(inner.chain_id),
                from,
            )
        }
        _ => return Err(DecodeError::RlpDecode("Unsupported transaction type".into())),
    };

    Ok(DecodedTransaction {
        to,
        value,
        input,
        from,
        chain_id,
    })
}

/// Parse claimAsset calldata from raw input bytes.
///
/// Returns the decoded ClaimAssetParams if the input is a valid claimAsset call,
/// or an error if the selector doesn't match or the data is malformed.
pub fn parse_claim_asset(input: &[u8]) -> Result<ClaimAssetParams, DecodeError> {
    if input.len() < 4 {
        return Err(DecodeError::NoInputData);
    }

    // Check selector
    if input[..4] != CLAIM_ASSET_SELECTOR {
        return Err(DecodeError::NotClaimAsset);
    }

    // Decode using alloy-sol-types
    let call = claimAssetCall::abi_decode(&input[4..], true)
        .map_err(|e| DecodeError::InvalidCalldata(e.to_string()))?;

    // Convert proof arrays
    let smt_proof_local: [[u8; 32]; 32] = call
        .smtProofLocalExitRoot
        .iter()
        .map(|b| b.0)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| DecodeError::InvalidCalldata("Invalid local proof length".into()))?;

    let smt_proof_rollup: [[u8; 32]; 32] = call
        .smtProofRollupExitRoot
        .iter()
        .map(|b| b.0)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| DecodeError::InvalidCalldata("Invalid rollup proof length".into()))?;

    Ok(ClaimAssetParams {
        smt_proof_local_exit_root: smt_proof_local,
        smt_proof_rollup_exit_root: smt_proof_rollup,
        global_index: GlobalIndex::decode(call.globalIndex),
        global_index_raw: call.globalIndex,
        mainnet_exit_root: call.mainnetExitRoot.0,
        rollup_exit_root: call.rollupExitRoot.0,
        origin_network: call.originNetwork,
        origin_token_address: call.originTokenAddress,
        destination_network: call.destinationNetwork,
        destination_address: call.destinationAddress,
        amount: call.amount,
        metadata: call.metadata,
    })
}

/// Check if the given calldata is a claimAsset call.
pub fn is_claim_asset(input: &[u8]) -> bool {
    input.len() >= 4 && input[..4] == CLAIM_ASSET_SELECTOR
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, FixedBytes, U256};
    use alloy_sol_types::SolCall;

    #[test]
    fn test_global_index_decode_mainnet() {
        // Mainnet flag set (bit 64 = 1), rollup_index = 0, local_root_index = 42
        // Binary: 1 << 64 | 42 = 0x10000000000000000 + 42
        let value = U256::from(1u128 << 64) + U256::from(42u64);
        let decoded = GlobalIndex::decode(value);

        assert!(decoded.mainnet_flag);
        assert_eq!(decoded.rollup_index, 0);
        assert_eq!(decoded.local_root_index, 42);
    }

    #[test]
    fn test_global_index_decode_rollup() {
        // Mainnet flag not set, rollup_index = 5, local_root_index = 100
        // rollup_index at bits 32-63: 5 << 32
        // local_root_index at bits 0-31: 100
        let value = U256::from((5u64 << 32) | 100);
        let decoded = GlobalIndex::decode(value);

        assert!(!decoded.mainnet_flag);
        assert_eq!(decoded.rollup_index, 5);
        assert_eq!(decoded.local_root_index, 100);
    }

    #[test]
    fn test_global_index_decode_all_fields() {
        // mainnet_flag = true, rollup_index = 3, local_root_index = 999
        let value = U256::from(1u128 << 64) + U256::from((3u64 << 32) | 999);
        let decoded = GlobalIndex::decode(value);

        assert!(decoded.mainnet_flag);
        assert_eq!(decoded.rollup_index, 3);
        assert_eq!(decoded.local_root_index, 999);
    }

    #[test]
    fn test_claim_asset_selector() {
        assert_eq!(CLAIM_ASSET_SELECTOR, [0xcc, 0xaa, 0x2d, 0x11]);
    }

    #[test]
    fn test_is_claim_asset() {
        // Valid selector
        let mut data = vec![0xcc, 0xaa, 0x2d, 0x11];
        data.extend_from_slice(&[0u8; 100]); // padding
        assert!(is_claim_asset(&data));

        // Wrong selector
        let wrong = vec![0x00, 0x00, 0x00, 0x00];
        assert!(!is_claim_asset(&wrong));

        // Too short
        let short = vec![0xcc, 0xaa];
        assert!(!is_claim_asset(&short));
    }

    #[test]
    fn test_not_claim_asset_error() {
        let wrong_selector = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x02];
        let result = parse_claim_asset(&wrong_selector);
        assert!(matches!(result, Err(DecodeError::NotClaimAsset)));
    }

    #[test]
    fn test_no_input_data_error() {
        let empty: [u8; 0] = [];
        let result = parse_claim_asset(&empty);
        assert!(matches!(result, Err(DecodeError::NoInputData)));

        let short = vec![0x2c, 0xff];
        let result = parse_claim_asset(&short);
        assert!(matches!(result, Err(DecodeError::NoInputData)));
    }

    #[test]
    fn test_parse_claim_asset_full() {
        // Create test data
        let smt_proof_local: [FixedBytes<32>; 32] = std::array::from_fn(|i| {
            FixedBytes::from([i as u8; 32])
        });
        let smt_proof_rollup: [FixedBytes<32>; 32] = std::array::from_fn(|i| {
            FixedBytes::from([(i + 100) as u8; 32])
        });

        // globalIndex: mainnet=false, rollup_index=7, local_root_index=42
        let global_index = U256::from((7u64 << 32) | 42);

        let mainnet_exit_root = FixedBytes::from([0xaa; 32]);
        let rollup_exit_root = FixedBytes::from([0xbb; 32]);
        let origin_network = 1u32;
        let origin_token = address!("0000000000000000000000000000000000000000");
        let destination_network = 2u32;
        let destination_address = address!("1111111111111111111111111111111111111111");
        let amount = U256::from(1000000000000000000u128); // 1 ETH
        let metadata = Bytes::from(vec![0x01, 0x02, 0x03]);

        // Encode the call
        let call = claimAssetCall {
            smtProofLocalExitRoot: smt_proof_local,
            smtProofRollupExitRoot: smt_proof_rollup,
            globalIndex: global_index,
            mainnetExitRoot: mainnet_exit_root,
            rollupExitRoot: rollup_exit_root,
            originNetwork: origin_network,
            originTokenAddress: origin_token,
            destinationNetwork: destination_network,
            destinationAddress: destination_address,
            amount,
            metadata: metadata.clone(),
        };

        let encoded = call.abi_encode();
        let mut calldata = CLAIM_ASSET_SELECTOR.to_vec();
        calldata.extend_from_slice(&encoded);

        // Parse it back
        let parsed = parse_claim_asset(&calldata).expect("Should parse valid calldata");

        // Verify all fields
        assert_eq!(parsed.global_index.mainnet_flag, false);
        assert_eq!(parsed.global_index.rollup_index, 7);
        assert_eq!(parsed.global_index.local_root_index, 42);
        assert_eq!(parsed.mainnet_exit_root, [0xaa; 32]);
        assert_eq!(parsed.rollup_exit_root, [0xbb; 32]);
        assert_eq!(parsed.origin_network, 1);
        assert_eq!(parsed.origin_token_address, origin_token);
        assert_eq!(parsed.destination_network, 2);
        assert_eq!(parsed.destination_address, destination_address);
        assert_eq!(parsed.amount, amount);
        assert_eq!(parsed.metadata.as_ref(), &[0x01, 0x02, 0x03]);

        // Verify SMT proofs
        for i in 0..32 {
            assert_eq!(parsed.smt_proof_local_exit_root[i], [i as u8; 32]);
            assert_eq!(parsed.smt_proof_rollup_exit_root[i], [(i + 100) as u8; 32]);
        }
    }

    #[test]
    fn test_parse_claim_asset_mainnet_origin() {
        // Test with mainnet flag set
        let smt_proof_local: [FixedBytes<32>; 32] = [FixedBytes::ZERO; 32];
        let smt_proof_rollup: [FixedBytes<32>; 32] = [FixedBytes::ZERO; 32];

        // globalIndex: mainnet=true, rollup_index=0 (ignored), local_root_index=123
        let global_index = U256::from(1u128 << 64) + U256::from(123u64);

        let call = claimAssetCall {
            smtProofLocalExitRoot: smt_proof_local,
            smtProofRollupExitRoot: smt_proof_rollup,
            globalIndex: global_index,
            mainnetExitRoot: FixedBytes::ZERO,
            rollupExitRoot: FixedBytes::ZERO,
            originNetwork: 0,
            originTokenAddress: Address::ZERO,
            destinationNetwork: 1,
            destinationAddress: address!("dead000000000000000000000000000000000000"),
            amount: U256::from(500),
            metadata: Bytes::new(),
        };

        let encoded = call.abi_encode();
        let mut calldata = CLAIM_ASSET_SELECTOR.to_vec();
        calldata.extend_from_slice(&encoded);

        let parsed = parse_claim_asset(&calldata).expect("Should parse");
        assert!(parsed.global_index.mainnet_flag);
        assert_eq!(parsed.global_index.rollup_index, 0);
        assert_eq!(parsed.global_index.local_root_index, 123);
    }

    #[test]
    fn test_claim_asset_selector_matches_abi() {
        // Verify our hardcoded selector matches what alloy computes
        let computed_selector = claimAssetCall::SELECTOR;
        assert_eq!(computed_selector, CLAIM_ASSET_SELECTOR);
    }
}
