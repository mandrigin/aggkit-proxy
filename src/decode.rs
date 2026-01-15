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
    // Note: abi_decode expects the FULL calldata including selector
    // Use validate=false for real-world transactions that may have non-canonical padding
    let call = claimAssetCall::abi_decode(input, false)
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

        // abi_encode() includes the selector
        let calldata = call.abi_encode();

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

        // abi_encode() includes the selector
        let calldata = call.abi_encode();

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

    /// Test parsing a real claimAsset transaction from Lumia mainnet.
    /// Transaction: https://explorer.lumia.org/tx/0x5fbc96fe75987cc4579591b27d608ff0732b4572e392d23e04431b7c4f9b5c54
    ///
    /// Expected values:
    /// - Global Index: 18446744073709784963 (0x10000000000038f83)
    /// - Origin Network: 0
    /// - Origin Token: 0xD9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7
    /// - Destination Network: 7
    /// - Destination Address: 0xA95867B23955F98a3A5774645DB2B603baba4003
    /// - Amount: 315000000000000000000 (315 LUMIA with 18 decimals)
    #[test]
    fn test_parse_real_lumia_claim_asset_tx() {
        // Real input data from Lumia mainnet transaction (fetched from explorer API)
        let input_hex = "ccaa2d112418ec50bf696531308dfff540e33203ae328484fe69e3d4817c2d3f3528b1270e75e3edcdedd0502bc6036556e72b7491273fc8356c3c2cfac822df97c3401bb4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d3021ddb9a356815c3fac1026b6dec5df3124afbadb485c9ba5a3e3398a04b7ba85e58769b32a1beaf1ea27375a44095a0d1fb664ce2dd358e7fcbfb78c26a193440eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d887c22bd8750d34016ac3c66b5ff102dacdd73f6b014e710b51e8022af9a1968f3247440b1b3fbe0d720167cf2c2eaeab34b71da84fb75a79a70a805d7402889021c363690844bd0ab092e47e2fbe02cd1072df76fcfd1828ffab0072ff07fe5636957efdf93be3b07f907be10cb84f556187fb94ddbcc2ec85648eb0c3d1ce111236c9d31d706677c869bcc6b7f48ad6ee82c27b68b5bdbd9e002c56db396c7f04165585496c873675f5acdcc4fb62983732988979dc5ac2adb5af19e7717ce3490c6ceeb450aecdc82e28293031d10c7d73bf85e57bf041a97360aa2c5d99cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a9000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000038f831dcb739b0d6b9c5c092b5b06a24d85f40918a469c360b382350c32ba011cafd9784b7ce061870cfc27aae91b0e6248a41430c450182c272ebf25bff4e561d7630000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a70000000000000000000000000000000000000000000000000000000000000007000000000000000000000000a95867b23955f98a3a5774645db2b603baba40030000000000000000000000000000000000000000000000111380cf0ef80c0000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000";

        let input_bytes = hex::decode(input_hex).expect("Valid hex input");

        // Verify it's a claimAsset call
        assert!(is_claim_asset(&input_bytes), "Should be claimAsset call");

        // Parse the calldata
        let parsed = parse_claim_asset(&input_bytes).expect("Should parse valid claimAsset calldata");

        // Verify decoded values match expected
        // Global Index: 0x10000000000038f83 = mainnet_flag=true, rollup_index=0, local_root_index=233347
        assert!(parsed.global_index.mainnet_flag, "Should be mainnet origin");
        assert_eq!(parsed.global_index.local_root_index, 233347, "local_root_index should be 233347 (0x38F83)");

        // Origin network: 0 (Ethereum mainnet)
        assert_eq!(parsed.origin_network, 0, "Origin network should be 0");

        // Origin token: 0xD9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7 (LUMIA token)
        let expected_origin_token = address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7");
        assert_eq!(parsed.origin_token_address, expected_origin_token, "Origin token should match LUMIA token address");

        // Destination network: 7 (Lumia L2)
        assert_eq!(parsed.destination_network, 7, "Destination network should be 7");

        // Destination address: 0xA95867B23955F98a3A5774645DB2B603baba4003
        let expected_dest = address!("A95867B23955F98a3A5774645DB2B603baba4003");
        assert_eq!(parsed.destination_address, expected_dest, "Destination address should match");

        // Amount: 315 LUMIA (315 * 10^18 = 315000000000000000000 = 0x111380cf0ef80c0000)
        let expected_amount = U256::from_str_radix("111380cf0ef80c0000", 16).unwrap();
        assert_eq!(parsed.amount, expected_amount, "Amount should be 315 LUMIA");

        // Verify metadata contains token info (name = "Lumia Token", symbol = "LUMIA", decimals = 18)
        assert!(!parsed.metadata.is_empty(), "Metadata should not be empty");
    }

    /// Test parsing multiple real claimAsset transactions from Lumia mainnet.
    /// All transactions have destination_network = 7 (Lumia L2).
    /// Origin token is LUMIA (0xD9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7).
    #[test]
    fn test_parse_multiple_lumia_claim_assets() {
        // Test vectors: raw_input_hex from real transactions
        let test_cases = [
            // TX 1: 0xe1a20811d757c48eba534f63041f58cd39eec762bfb6e4496dccf4e675fd5619
            "ccaa2d11bdb609e9b9f2dfcb3c9ea71ac3f01b0c9ab3b37747f9d0b87e0c61596a9c257d3d2339c39ddd4267ae4631a46c638cf51b49b717c4f8bfd37ed1b66530914752b4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d304ba63ee0fdd3c072b419435a10a294ba515d2112690c19a253e9db1b5537dde8bf15a90a29aac793715881f7041a2898fa290a2dda6e342cf97c9d1db34f215baff92b0cb72d5d4d61e2027f5365d1c2f50a498a0e544c96790c1ee3994de61894f694e710a8de24663f875a0369c04be1fd9972ad7e97f0ffa433edd553a1192c0cac9f42b7abcc62fdafb18be6a1a35d4186a84ac37cad49769d7bcd30a6bb3084a507526fca1be690e2cd82060571d7cda42205ea47827107f702b4862f63cefad4e508c098b9a7e1d8feb19955fb02ba9675585078710969d3440f5054e0a511ed0865c34b826c82a7b15cf0a13802ff11cd933ee5e49c863f82316c9189f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000395fb98c911b6dcface93fd0bb490d09390f2f7f9fcf36fc208cbb36528a2292983266a2533a24cc2a3feecf5c09b6a270bbb24a5e2ce02c18c0e26cd54c3dddc2d700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a70000000000000000000000000000000000000000000000000000000000000007000000000000000000000000d9b20fe633b609b01081ad0428e81f8dd604f5c500000000000000000000000000000000000000000000065a31e0efc8c7f80000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 2: 0xe64254ff002b3d46b46af077fa24c6ef5b54d950759d70d6d9a693b1d36de188
            "ccaa2d110000000000000000000000000000000000000000000000000000000000000000759ec28c1a0ada9c923de5492f5f769f04ccc227836ba44d4b05cbba1f5989454dd98d9c09bbca336145d379f303de0d4928674b5980cd1d3690028af8bbad34e5766f036b36d3d7f278386ea5f37d13de015c8f03dda3f6904be52fe688d9ede6276d7d7ab49d95565ac163e33483c3a1d0489cad1e171a55079a6645b8133f0eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d94f694e710a8de24663f875a0369c04be1fd9972ad7e97f0ffa433edd553a1192c0cac9f42b7abcc62fdafb18be6a1a35d4186a84ac37cad49769d7bcd30a6bb3084a507526fca1be690e2cd82060571d7cda42205ea47827107f702b4862f63cefad4e508c098b9a7e1d8feb19955fb02ba9675585078710969d3440f5054e0a511ed0865c34b826c82a7b15cf0a13802ff11cd933ee5e49c863f82316c9189f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000395debb71d991caf89fe64878259a61ae8d0b4310c176e66d90fd2370b02573e80c90d9b546933b59acd388dc0c6520cbf2d4dbb9bac66f74f167ba70f221d82a440c0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a70000000000000000000000000000000000000000000000000000000000000007000000000000000000000000d9b20fe633b609b01081ad0428e81f8dd604f5c50000000000000000000000000000000000000000000000001bc16d674ec80000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 3: 0xd092dcf22398669262773cdd0e9749a5c01c75eab04f79d815566be041391c7c
            "ccaa2d1100000000000000000000000000000000000000000000000000000000000000009810c211f0e9b4021576d32dc597dffca346b2688b764a6468ea4578e88ac7d8a3a6325a240517053fe301d4f20d8550117fe9fbec080ca3c30d83037791281121ddb9a356815c3fac1026b6dec5df3124afbadb485c9ba5a3e3398a04b7ba85e58769b32a1beaf1ea27375a44095a0d1fb664ce2dd358e7fcbfb78c26a193440eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d12921a12d758160db0adf53558e3e28e805fb140ad7739bec03314659ff7b5f4ffd70157e48063fc33c97a050f7f640233bf646cc98d9524c6b92bcf3ab56f839867cc5f7f196b93bae1e27e6320742445d290f2263827498b54fec539f756afcefad4e508c098b9a7e1d8feb19955fb02ba9675585078710969d3440f5054e0a511ed0865c34b826c82a7b15cf0a13802ff11cd933ee5e49c863f82316c9189f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a9000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000039446ee68132075ba3b8da133ec747dca74b1394c53a1c88f9723bc4088a8270decff4700405b1d6886e3ac5b51c305b5a6b20f3bbee145b59b7a3afe5e7035938ea80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a700000000000000000000000000000000000000000000000000000000000000070000000000000000000000004592b9b6e1731c3d052f5dc1904c4b7f31433eac000000000000000000000000000000000000000000021165a6a9ff3ba63c0000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 4: 0x73eb9910ff4cfdbd5d2bb77d1c020b82112765bfded64953131eaaef331f6c59
            "ccaa2d11dd213f60467e7a3c01c39033f3c26173d780b76e1c2ef19ffcaa0c3e6508bbfbc05fd87b551a405eff13e9798e61385af5152517d35bd1071884786032d1b9a9c56c4f99de3676b6c129182d5bb8b2c5bdc822e2e70b3c05b5a941541ec1aa398defc0b765239d4eb2e01f775e7a01865b54ef8cfa4a70ccba248f35e44f463a71e5f4836bafb20b0313c2f75425c2ea42545a8fcd55ce8677c95371b533002b0eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d0ca698b67a5e69f013519ec65852fef163d573998b37532f5b253c0b2df4323bf406da48fb22512a9044f6241a16068635f8961abbd5aeba5fb09af448c696a4d9faea4a86009232b23e2c3ad1484ea9a185a000497a8554f610769a0946bcd48d2cb0ef3f6255b35c543a00d899ffd74dc93f13a7b63c9fe7767a6800529465f9dc3e7fe016e050eff260334f18a5d4fe391d82092319f5964f2e2eb7c1c3a5f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000393dbf0c831429c86b5214df472aef6727517c7facc57b20ca0c15f8d6a55ec10aa460b8c73dc90c232eec876662c22172aa1e05323900a21d87f3bc2611b0a36ed8b0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a700000000000000000000000000000000000000000000000000000000000000070000000000000000000000002916d4e08d06fb0bd5c326374bdf5613c099ecca00000000000000000000000000000000000000000000000013c17a949e24a2fc000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 5: 0x01a1886dc28a57d4d1b6e7f68e4f1fc35210276687942126b334ee636070c8df
            "ccaa2d11d4365179152d32601e39ef35253e1b4c722ec6395827fa55e4290931985f3d0dad3228b676f7d3cd4284a5443f17f1962b36e491b30a40b2405849e597ba5fb588252e035d2b781695cf51df27564959785f14d08ae694caa4b1a7866fecbe1021ddb9a356815c3fac1026b6dec5df3124afbadb485c9ba5a3e3398a04b7ba8571e5f4836bafb20b0313c2f75425c2ea42545a8fcd55ce8677c95371b533002b0eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d0ca698b67a5e69f013519ec65852fef163d573998b37532f5b253c0b2df4323bf406da48fb22512a9044f6241a16068635f8961abbd5aeba5fb09af448c696a4d9faea4a86009232b23e2c3ad1484ea9a185a000497a8554f610769a0946bcd48d2cb0ef3f6255b35c543a00d899ffd74dc93f13a7b63c9fe7767a6800529465f9dc3e7fe016e050eff260334f18a5d4fe391d82092319f5964f2e2eb7c1c3a5f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000393d5191c18be915a9c562faca7ddef2ad366bb3c1ec1fe3b7d67e2a126f620ef46952f9c99f5514dd04c2c25d4968350c588643cd7a082a5ad58844c1f45b88e80dc0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a700000000000000000000000000000000000000000000000000000000000000070000000000000000000000004a893e8d6fbe6084e30553a2fc2ff5eb775bfb20000000000000000000000000000000000000000000000000200a96492437d3ce000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 6: 0x6a2968ec482284787884319b702dab446cff6489bd27f63e1963cded33f6e253
            "ccaa2d118850f4e939b98c133daad51f1cb0471911a188863d3c7f9b25d95ec708e973a870e5d83e5f1f4eb6580cb120ba12bb7affc84f91140edb836d57cdcaf1025b3bb4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d3021ddb9a356815c3fac1026b6dec5df3124afbadb485c9ba5a3e3398a04b7ba8571e5f4836bafb20b0313c2f75425c2ea42545a8fcd55ce8677c95371b533002b0eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d0ca698b67a5e69f013519ec65852fef163d573998b37532f5b253c0b2df4323bf406da48fb22512a9044f6241a16068635f8961abbd5aeba5fb09af448c696a4d9faea4a86009232b23e2c3ad1484ea9a185a000497a8554f610769a0946bcd48d2cb0ef3f6255b35c543a00d899ffd74dc93f13a7b63c9fe7767a6800529465f9dc3e7fe016e050eff260334f18a5d4fe391d82092319f5964f2e2eb7c1c3a5f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000393d33156dd7e34f565f331d1aa5cfa138d93e74006cbee9f1628eff980a605078a732f9c99f5514dd04c2c25d4968350c588643cd7a082a5ad58844c1f45b88e80dc0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a7000000000000000000000000000000000000000000000000000000000000000700000000000000000000000058685c0ce0585ed17adff660fc5c0774fbd9c57f00000000000000000000000000000000000000000000000020148cc39f073783000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 7: 0xb9f3c3f74be9d852d98da469db4a1bd21ee03e945301a129197bb65a19a24fca
            "ccaa2d110000000000000000000000000000000000000000000000000000000000000000ad3228b676f7d3cd4284a5443f17f1962b36e491b30a40b2405849e597ba5fb5b4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d3021ddb9a356815c3fac1026b6dec5df3124afbadb485c9ba5a3e3398a04b7ba8571e5f4836bafb20b0313c2f75425c2ea42545a8fcd55ce8677c95371b533002b0eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d0ca698b67a5e69f013519ec65852fef163d573998b37532f5b253c0b2df4323bf406da48fb22512a9044f6241a16068635f8961abbd5aeba5fb09af448c696a4d9faea4a86009232b23e2c3ad1484ea9a185a000497a8554f610769a0946bcd48d2cb0ef3f6255b35c543a00d899ffd74dc93f13a7b63c9fe7767a6800529465f9dc3e7fe016e050eff260334f18a5d4fe391d82092319f5964f2e2eb7c1c3a5f8b13a49e282f609c317a833fb8d976d11517c571d1221a265d25af778ecf8925a925caf7bfdf31344037ba5b42657130d049f7cb9e87877317e79fce2543a0cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000393d05ad4b42b66022b60edc445ddfbe57178970c6bf4a6a4932958b5bb589da7ec54b95a00ee00220482f70eddd89a93c8813d6d8d72e487ab2ee6a0c2f462e39aa20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a70000000000000000000000000000000000000000000000000000000000000007000000000000000000000000690800299cfcf3a5e21c5364bfba9d478af5c6590000000000000000000000000000000000000000000000000dd229985f1716b9000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 8: 0x5fbc96fe75987cc4579591b27d608ff0732b4572e392d23e04431b7c4f9b5c54
            "ccaa2d112418ec50bf696531308dfff540e33203ae328484fe69e3d4817c2d3f3528b1270e75e3edcdedd0502bc6036556e72b7491273fc8356c3c2cfac822df97c3401bb4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d3021ddb9a356815c3fac1026b6dec5df3124afbadb485c9ba5a3e3398a04b7ba85e58769b32a1beaf1ea27375a44095a0d1fb664ce2dd358e7fcbfb78c26a193440eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d887c22bd8750d34016ac3c66b5ff102dacdd73f6b014e710b51e8022af9a1968f3247440b1b3fbe0d720167cf2c2eaeab34b71da84fb75a79a70a805d7402889021c363690844bd0ab092e47e2fbe02cd1072df76fcfd1828ffab0072ff07fe5636957efdf93be3b07f907be10cb84f556187fb94ddbcc2ec85648eb0c3d1ce111236c9d31d706677c869bcc6b7f48ad6ee82c27b68b5bdbd9e002c56db396c7f04165585496c873675f5acdcc4fb62983732988979dc5ac2adb5af19e7717ce3490c6ceeb450aecdc82e28293031d10c7d73bf85e57bf041a97360aa2c5d99cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a9000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000038f831dcb739b0d6b9c5c092b5b06a24d85f40918a469c360b382350c32ba011cafd9784b7ce061870cfc27aae91b0e6248a41430c450182c272ebf25bff4e561d7630000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a70000000000000000000000000000000000000000000000000000000000000007000000000000000000000000a95867b23955f98a3a5774645db2b603baba40030000000000000000000000000000000000000000000000111380cf0ef80c0000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 9: 0xd3aa428a9bdc103bc3c65c47476e8262cc95fd246314af3f4e66f4fdf68eac32
            "ccaa2d113f1e27d064b06689de9b9d93138e04635531d1aa4c84f702b4303c39f26134a37fabf9424213f87e028dbf8d71a9da53fcb28334ee95a12c0aed60272517be60b4c11951957c6f8f642c4af61cd6b24640fec6dc7fc607ee8206a99e92410d300d208274ece855b66a21b29c3af029409f1bd48a5d12fad2617e44cce5ec92f1e58769b32a1beaf1ea27375a44095a0d1fb664ce2dd358e7fcbfb78c26a193440eb01ebfc9ed27500cd4dfc979272d1f0913cc9f66540d7e8005811109e1cf2d887c22bd8750d34016ac3c66b5ff102dacdd73f6b014e710b51e8022af9a196886634f2260e7d85b74829a0e086f5a19f4ab70b5d69b8f765f0a6f828a14a5a69867cc5f7f196b93bae1e27e6320742445d290f2263827498b54fec539f756af636957efdf93be3b07f907be10cb84f556187fb94ddbcc2ec85648eb0c3d1ce111236c9d31d706677c869bcc6b7f48ad6ee82c27b68b5bdbd9e002c56db396c7f04165585496c873675f5acdcc4fb62983732988979dc5ac2adb5af19e7717ce3490c6ceeb450aecdc82e28293031d10c7d73bf85e57bf041a97360aa2c5d99cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a9000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000038e8b4d401f8878f0f162f36c62e868a3eec9250b272dd691a99c2123230b75504b25ae0354ef27c7c2f8adbfc60351cce9de7fcaad76619ad46dc7759d43e681e7730000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a7000000000000000000000000000000000000000000000000000000000000000700000000000000000000000036b511931e8d91142594e16ec392f0c5156df4cf000000000000000000000000000000000000000000000005ce0894aa74d40000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
            // TX 10: 0xefd30651db399351518b541b4e1371f01595b8c7b5beb073605f6825b20967d5
            "ccaa2d114c67864beb2c4e027bda95d477ad2e3a943628847708edbf129a42f74d044a4f53b43a130e8a6f8d1cc8324ee6ea80e755bedcad8276ebadbcd7c2f147dea67854324a88bbac0394d4221a531c939b808bd4c2878558ddc178cd0813b5f4017f16c49231ab0b9c12e812e68a96b77ba0110dda27f893eea7313a9747078633b9f218d6b2216c831141f15c1bceb7d6977b5b48ec88bf810357c4c9cc9d72ac73683a690a79c361f90e37318a7f39dbb5171607bade09478ce35f149fd605beb4f9ea1eb66fa28468c6c302801a3fb3f34999279eb39333d4aefa321b633a2e98ffd70157e48063fc33c97a050f7f640233bf646cc98d9524c6b92bcf3ab56f839867cc5f7f196b93bae1e27e6320742445d290f2263827498b54fec539f756afcefad4e508c098b9a7e1d8feb19955fb02ba9675585078710969d3440f5054e0f9dc3e7fe016e050eff260334f18a5d4fe391d82092319f5964f2e2eb7c1c3a5f04165585496c873675f5acdcc4fb62983732988979dc5ac2adb5af19e7717ce3490c6ceeb450aecdc82e28293031d10c7d73bf85e57bf041a97360aa2c5d99cc1df82d9c4b87413eae2ef048f94b4d3554cea73d92b0f7af96e0271c691e2bb5c67add7c6caf302256adedf7ab114da0acfe870d449a3a489f781d659e8becc4111a1a05cc06ad682bb0f213170d7d57049920d20fc4e0f7556a21b283a7e2a77a0f8b0e0b4e5a57f5e381b3892bb41a0bcdbfdf3c7d591fae02081159b594d361122b4b1d18ab577f2aeb6632c690713456a66a5670649ceb2c0a31e43ab465a2dce0a8a7f68bb74560f8f71837c2c2ebbcbf7fffb42ae1896f13f7c7479a0b46a28b6f55540f89444f63de0378e3d121be09e06cc9ded1c20e65876d36aa0c65e9645644786b620e2dd2ad648ddfcbf4a7e5b1a3a4ecfe7f64667a3f0b7e2f4418588ed35a2458cffeb39b93d26f18d2ab13bdce6aee58e7b99359ec2dfd95a9c16dc00d6ef18b7933a6f8dc65ccb55667138776f7dea101070dc8796e3774df84f40ae0c8229d0d6069e5c8f39a7c299677a09d367fc7b05e3bc380ee652cdc72595f74c7b1043d0e1ffbab734648c838dfb0527d971b602bc216c9619ef0abf5ac974a1ed57f4050aa510dd9c74f508277b39d7973bb2dfccc5eeb0618db8cd74046ff337f0a7bf2c8e03e10f642c1886798d71806ab1e888d9e5ee87d0838c5655cb21c6cb83313b5a631175dff4963772cce9108188b34ac87c81c41e662ee4dd2dd7b2bc707961b1e646c4047669dcb6584f0d8d770daf5d7e7deb2e388ab20e2573d171a88108e79d820e98f26c0b84aa8b2f4aa4968dbb818ea32293237c50ba75ee485f4c22adf2f741400bdf8d6a9cc7df7ecae576221665d7358448818bb4ae4562849e949e17ac16e0be16688e156b5cf15e098c627c0056a900000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000000003883ffcca2b92792468b75e50a9d93e755b69c9c305b367b23e86830939e9bdb5e18c6a5dd9bbe7d3dfe18820ab82e43d388426b8a3aba8349550cdcfaf54048b6a900000000000000000000000000000000000000000000000000000000000000000000000000000000000000000d9343a049d5dbd89cd19dc6bca8c48fb3a0a42a70000000000000000000000000000000000000000000000000000000000000007000000000000000000000000c339bb03c0e1cc710f12ae5831f87612c3cb7960000000000000000000000000000000000000000000000ea67ff309c194730000000000000000000000000000000000000000000000000000000000000000092000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000012000000000000000000000000000000000000000000000000000000000000000b4c756d696120546f6b656e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000054c554d4941000000000000000000000000000000000000000000000000000000",
        ];

        let expected_dest_network = 7u32;
        let expected_origin_token = address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7");

        for (i, input_hex) in test_cases.iter().enumerate() {
            let input_bytes = hex::decode(input_hex).expect("Valid hex input");

            // Verify selector
            assert!(is_claim_asset(&input_bytes), "TX {} should be claimAsset call", i + 1);

            // Parse
            let parsed = parse_claim_asset(&input_bytes)
                .unwrap_or_else(|e| panic!("TX {} should parse: {:?}", i + 1, e));

            // Verify destination network is always 7 (Lumia L2)
            assert_eq!(
                parsed.destination_network, expected_dest_network,
                "TX {} destination_network should be 7", i + 1
            );

            // Verify origin token is LUMIA
            assert_eq!(
                parsed.origin_token_address, expected_origin_token,
                "TX {} origin_token should be LUMIA", i + 1
            );

            // Verify origin network is 0 (Ethereum mainnet)
            assert_eq!(
                parsed.origin_network, 0,
                "TX {} origin_network should be 0 (Ethereum)", i + 1
            );

            // Verify mainnet flag is set (bridging from mainnet)
            assert!(
                parsed.global_index.mainnet_flag,
                "TX {} should have mainnet_flag set", i + 1
            );
        }
    }

    /// Test decoding RLP-encoded signed transactions containing claimAsset calldata.
    /// This tests the decode_transaction() function which handles full Ethereum transactions.
    #[test]
    fn test_decode_rlp_signed_claim_asset_transaction() {
        use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
        use alloy_primitives::{Bytes, PrimitiveSignature, TxKind};
        use alloy_rlp::Encodable;
        use alloy_sol_types::SolCall;

        // Test private key (well-known test key from hardhat/foundry)
        // Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
        let private_key_bytes: [u8; 32] = [
            0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38,
            0xff, 0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b,
            0xf4, 0xf2, 0xff, 0x80,
        ];
        let signing_key =
            k256::ecdsa::SigningKey::from_bytes(&private_key_bytes.into()).expect("valid key");
        let expected_signer = address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266");

        // Bridge contract address
        let bridge_contract = address!("2a3DD3EB832aF982ec71669E178424b10Dca2EDe");

        // Create claimAsset calldata
        let smt_proof_local: [FixedBytes<32>; 32] = [FixedBytes::ZERO; 32];
        let smt_proof_rollup: [FixedBytes<32>; 32] = [FixedBytes::ZERO; 32];
        let global_index = U256::from(1u128 << 64) + U256::from(12345u64); // mainnet, local_root_index=12345

        let call = claimAssetCall {
            smtProofLocalExitRoot: smt_proof_local,
            smtProofRollupExitRoot: smt_proof_rollup,
            globalIndex: global_index,
            mainnetExitRoot: FixedBytes::ZERO,
            rollupExitRoot: FixedBytes::ZERO,
            originNetwork: 0,
            originTokenAddress: address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7"),
            destinationNetwork: 7,
            destinationAddress: address!("A95867B23955F98a3A5774645DB2B603baba4003"),
            amount: U256::from(1000000000000000000u128), // 1 token
            metadata: Bytes::new(),
        };
        let calldata = call.abi_encode();

        // Create EIP-1559 transaction
        let tx = TxEip1559 {
            chain_id: 1,
            nonce: 42,
            gas_limit: 500_000,
            max_fee_per_gas: 50_000_000_000,
            max_priority_fee_per_gas: 1_000_000_000,
            to: TxKind::Call(bridge_contract),
            value: U256::ZERO,
            access_list: Default::default(),
            input: Bytes::from(calldata.clone()),
        };

        // Sign the transaction
        let signature_hash = tx.signature_hash();
        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(signature_hash.as_slice())
            .expect("signing should succeed");

        let prim_sig =
            PrimitiveSignature::from_signature_and_parity(signature, recovery_id.is_y_odd());
        let signed = TxEnvelope::Eip1559(alloy_consensus::Signed::new_unchecked(
            tx,
            prim_sig,
            signature_hash,
        ));

        // RLP encode
        let mut rlp_bytes = Vec::new();
        signed.encode(&mut rlp_bytes);

        // Now test our decode_transaction function
        let decoded = decode_transaction(&rlp_bytes).expect("Should decode RLP transaction");

        // Verify decoded fields
        assert_eq!(decoded.from, expected_signer, "Signer should be recovered correctly");
        assert_eq!(decoded.to, Some(bridge_contract), "To address should match");
        assert_eq!(decoded.value, U256::ZERO, "Value should be zero");
        assert_eq!(decoded.chain_id, Some(1), "Chain ID should be 1");
        assert_eq!(decoded.input.as_ref(), calldata.as_slice(), "Input should match calldata");

        // Verify the input is a valid claimAsset call
        assert!(is_claim_asset(&decoded.input), "Should be claimAsset call");

        // Parse the claimAsset parameters from the decoded transaction
        let parsed = parse_claim_asset(&decoded.input).expect("Should parse claimAsset");
        assert!(parsed.global_index.mainnet_flag, "Should be mainnet origin");
        assert_eq!(parsed.global_index.local_root_index, 12345);
        assert_eq!(parsed.destination_network, 7);
        assert_eq!(
            parsed.origin_token_address,
            address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7")
        );
    }

    /// Test decoding different transaction types (Legacy, EIP-2930, EIP-1559).
    #[test]
    fn test_decode_different_tx_types() {
        use alloy_consensus::{SignableTransaction, TxEip1559, TxEip2930, TxLegacy, TxEnvelope};
        use alloy_primitives::{Bytes, PrimitiveSignature, TxKind};
        use alloy_rlp::Encodable;
        use alloy_sol_types::SolCall;

        let private_key_bytes: [u8; 32] = [
            0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38,
            0xff, 0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b,
            0xf4, 0xf2, 0xff, 0x80,
        ];
        let signing_key =
            k256::ecdsa::SigningKey::from_bytes(&private_key_bytes.into()).expect("valid key");
        let expected_signer = address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        let bridge_contract = address!("2a3DD3EB832aF982ec71669E178424b10Dca2EDe");

        // Create minimal claimAsset calldata
        let call = claimAssetCall {
            smtProofLocalExitRoot: [FixedBytes::ZERO; 32],
            smtProofRollupExitRoot: [FixedBytes::ZERO; 32],
            globalIndex: U256::from(1u128 << 64),
            mainnetExitRoot: FixedBytes::ZERO,
            rollupExitRoot: FixedBytes::ZERO,
            originNetwork: 0,
            originTokenAddress: Address::ZERO,
            destinationNetwork: 7,
            destinationAddress: expected_signer,
            amount: U256::from(100),
            metadata: Bytes::new(),
        };
        let calldata = Bytes::from(call.abi_encode());

        // Test Legacy transaction
        {
            let tx = TxLegacy {
                chain_id: Some(1),
                nonce: 0,
                gas_price: 20_000_000_000,
                gas_limit: 500_000,
                to: TxKind::Call(bridge_contract),
                value: U256::ZERO,
                input: calldata.clone(),
            };

            let sig_hash = tx.signature_hash();
            let (sig, rec_id) = signing_key
                .sign_prehash_recoverable(sig_hash.as_slice())
                .expect("sign");
            let prim_sig = PrimitiveSignature::from_signature_and_parity(sig, rec_id.is_y_odd());
            let signed = TxEnvelope::Legacy(alloy_consensus::Signed::new_unchecked(
                tx, prim_sig, sig_hash,
            ));

            let mut rlp = Vec::new();
            signed.encode(&mut rlp);

            let decoded = decode_transaction(&rlp).expect("decode legacy");
            assert_eq!(decoded.from, expected_signer);
            assert_eq!(decoded.to, Some(bridge_contract));
            assert!(is_claim_asset(&decoded.input));
        }

        // Test EIP-2930 transaction
        {
            let tx = TxEip2930 {
                chain_id: 1,
                nonce: 1,
                gas_price: 20_000_000_000,
                gas_limit: 500_000,
                to: TxKind::Call(bridge_contract),
                value: U256::ZERO,
                input: calldata.clone(),
                access_list: Default::default(),
            };

            let sig_hash = tx.signature_hash();
            let (sig, rec_id) = signing_key
                .sign_prehash_recoverable(sig_hash.as_slice())
                .expect("sign");
            let prim_sig = PrimitiveSignature::from_signature_and_parity(sig, rec_id.is_y_odd());
            let signed = TxEnvelope::Eip2930(alloy_consensus::Signed::new_unchecked(
                tx, prim_sig, sig_hash,
            ));

            let mut rlp = Vec::new();
            signed.encode(&mut rlp);

            let decoded = decode_transaction(&rlp).expect("decode eip2930");
            assert_eq!(decoded.from, expected_signer);
            assert_eq!(decoded.to, Some(bridge_contract));
            assert_eq!(decoded.chain_id, Some(1));
            assert!(is_claim_asset(&decoded.input));
        }

        // Test EIP-1559 transaction
        {
            let tx = TxEip1559 {
                chain_id: 1,
                nonce: 2,
                max_fee_per_gas: 50_000_000_000,
                max_priority_fee_per_gas: 1_000_000_000,
                gas_limit: 500_000,
                to: TxKind::Call(bridge_contract),
                value: U256::ZERO,
                input: calldata,
                access_list: Default::default(),
            };

            let sig_hash = tx.signature_hash();
            let (sig, rec_id) = signing_key
                .sign_prehash_recoverable(sig_hash.as_slice())
                .expect("sign");
            let prim_sig = PrimitiveSignature::from_signature_and_parity(sig, rec_id.is_y_odd());
            let signed = TxEnvelope::Eip1559(alloy_consensus::Signed::new_unchecked(
                tx, prim_sig, sig_hash,
            ));

            let mut rlp = Vec::new();
            signed.encode(&mut rlp);

            let decoded = decode_transaction(&rlp).expect("decode eip1559");
            assert_eq!(decoded.from, expected_signer);
            assert_eq!(decoded.to, Some(bridge_contract));
            assert_eq!(decoded.chain_id, Some(1));
            assert!(is_claim_asset(&decoded.input));
        }
    }
}
