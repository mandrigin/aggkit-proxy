//! Generate RLP-encoded signed transactions from claimAsset calldata.
//!
//! This example creates signed Ethereum transactions wrapping claimAsset calldata,
//! which can be used to test the RLP decode path in the proxy.
//!
//! Usage:
//!   cargo run --example gen_signed_tx
//!
//! The output is hex-encoded RLP transactions ready for eth_sendRawTransaction.

use alloy_consensus::{SignableTransaction, TxEip1559};
use alloy_primitives::{address, hex, Bytes, FixedBytes, PrimitiveSignature, TxKind, U256};
use alloy_rlp::Encodable;
use alloy_sol_types::{sol, SolCall};

// Define the claimAsset function ABI
sol! {
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

fn main() {
    // Test private key (DO NOT use in production!)
    // This is a well-known test key: 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80
    // Corresponding address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
    let private_key_bytes: [u8; 32] = [
        0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38, 0xff,
        0x94, 0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b, 0xf4, 0xf2,
        0xff, 0x80,
    ];

    let signing_key =
        k256::ecdsa::SigningKey::from_bytes(&private_key_bytes.into()).expect("valid signing key");

    // Bridge contract address (placeholder - use real address in production)
    let bridge_contract = address!("2a3DD3EB832aF982ec71669E178424b10Dca2EDe");

    // Chain ID for Ethereum mainnet
    let chain_id: u64 = 1;

    println!("# RLP-Encoded Signed claimAsset Transactions");
    println!("# Generated from programmatic calldata");
    println!("# Signer: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 (test key)");
    println!("# Chain ID: {}", chain_id);
    println!("# Bridge Contract: {}", bridge_contract);
    println!();

    // Generate test transactions with varying parameters
    let test_cases = vec![
        // Test case 1: Basic mainnet claim
        (
            U256::from(1u128 << 64) + U256::from(12345u64), // mainnet, local_root_index=12345
            address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7"), // LUMIA token
            address!("A95867B23955F98a3A5774645DB2B603baba4003"), // destination
            U256::from(1000000000000000000u128), // 1 token
        ),
        // Test case 2: Different local root index
        (
            U256::from(1u128 << 64) + U256::from(233347u64), // mainnet, local_root_index=233347
            address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7"), // LUMIA token
            address!("d9b20fe633b609b01081ad0428e81f8dd604f5c5"), // destination
            U256::from(315000000000000000000u128), // 315 tokens
        ),
        // Test case 3: Small amount
        (
            U256::from(1u128 << 64) + U256::from(100u64), // mainnet, local_root_index=100
            address!("D9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7"), // LUMIA token
            address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266"), // destination (signer)
            U256::from(100), // 100 wei
        ),
    ];

    for (i, (global_index, origin_token, destination, amount)) in test_cases.iter().enumerate() {
        // Create claimAsset calldata
        let call = claimAssetCall {
            smtProofLocalExitRoot: [FixedBytes::ZERO; 32],
            smtProofRollupExitRoot: [FixedBytes::ZERO; 32],
            globalIndex: *global_index,
            mainnetExitRoot: FixedBytes::ZERO,
            rollupExitRoot: FixedBytes::ZERO,
            originNetwork: 0,
            originTokenAddress: *origin_token,
            destinationNetwork: 7,
            destinationAddress: *destination,
            amount: *amount,
            metadata: Bytes::new(),
        };
        let calldata = call.abi_encode();

        // Create EIP-1559 transaction
        let tx = TxEip1559 {
            chain_id,
            nonce: i as u64,
            gas_limit: 500_000,
            max_fee_per_gas: 50_000_000_000,    // 50 gwei
            max_priority_fee_per_gas: 1_000_000_000, // 1 gwei
            to: TxKind::Call(bridge_contract),
            value: U256::ZERO,
            access_list: Default::default(),
            input: Bytes::from(calldata),
        };

        // Sign the transaction
        let signature_hash = tx.signature_hash();
        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(signature_hash.as_slice())
            .expect("signing should succeed");

        let prim_sig =
            PrimitiveSignature::from_signature_and_parity(signature, recovery_id.is_y_odd());
        let signed = alloy_consensus::TxEnvelope::Eip1559(
            alloy_consensus::Signed::new_unchecked(tx, prim_sig, signature_hash),
        );

        // RLP encode
        let mut buf = Vec::new();
        signed.encode(&mut buf);

        println!("# TX {} (nonce={}, dest={})", i + 1, i, destination);
        println!("0x{}", hex::encode(&buf));
        println!();
    }
}
