//! Asset mapping module for faucet selection based on origin network/token.
//!
//! This module provides functionality to map incoming bridge claims to the appropriate
//! faucet based on the origin network and token address.
//!
//! # Current Mappings
//!
//! | Origin Network | Origin Token | Asset Symbol | Notes |
//! |----------------|--------------|--------------|-------|
//! | 0 (Ethereum)   | 0x00...00    | WETH         | Native ETH wrapped |
//! | Unknown        | Unknown      | UNKNOWN      | Warning logged |
//!
//! # Future Extensions
//!
//! This module is designed to be extended with:
//! - Additional token mappings (USDC, USDT, etc.)
//! - Configurable mappings via config file
//! - Per-asset faucet ID lookup (when multiple faucets are supported)

use tracing::warn;

/// Zero address constant (20 zero bytes) - represents native ETH/WETH
const ZERO_ADDRESS: [u8; 20] = [0u8; 20];

/// Asset symbol for Wrapped ETH (native ETH bridged via zero address)
pub const SYMBOL_WETH: &str = "WETH";

/// Asset symbol for unknown/unrecognized tokens
pub const SYMBOL_UNKNOWN: &str = "UNKNOWN";

/// Result of looking up an asset based on origin network and token.
#[derive(Debug, Clone)]
pub struct AssetLookupResult {
    /// The asset symbol (e.g., "WETH", "UNKNOWN")
    pub symbol: &'static str,
    /// Whether this is a known/recognized asset
    pub is_known: bool,
}

/// Look up the asset symbol based on origin network and token address.
///
/// # Arguments
///
/// * `origin_network` - The network ID where the token originated (0 = Ethereum)
/// * `origin_token` - The token contract address on the origin network
///
/// # Returns
///
/// Returns `AssetLookupResult` with:
/// - `symbol`: The asset symbol for logging/display
/// - `is_known`: Whether this is a recognized token mapping
///
/// # Known Mappings
///
/// - Network 0, Token 0x00...00 (zero address) → WETH (native ETH)
///
/// # Example
///
/// ```ignore
/// let result = lookup_asset(0, &[0u8; 20]);
/// assert_eq!(result.symbol, "WETH");
/// assert!(result.is_known);
/// ```
pub fn lookup_asset(origin_network: u32, origin_token: &[u8; 20]) -> AssetLookupResult {
    match (origin_network, origin_token) {
        // WETH: Ethereum (network 0) + zero address (native ETH)
        (0, token) if token == &ZERO_ADDRESS => AssetLookupResult {
            symbol: SYMBOL_WETH,
            is_known: true,
        },

        // Unknown token - log warning and return UNKNOWN symbol
        (network, token) => {
            warn!(
                origin_network = network,
                origin_token = %hex::encode(token),
                "Unknown origin token - mapping to UNKNOWN asset. \
                 Consider adding this token to the asset mapping configuration."
            );
            AssetLookupResult {
                symbol: SYMBOL_UNKNOWN,
                is_known: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weth_mapping() {
        // WETH: network 0, zero address
        let result = lookup_asset(0, &[0u8; 20]);
        assert_eq!(result.symbol, SYMBOL_WETH);
        assert!(result.is_known);
    }

    #[test]
    fn test_unknown_token_different_network() {
        // Unknown: non-zero network with zero address
        let result = lookup_asset(1, &[0u8; 20]);
        assert_eq!(result.symbol, SYMBOL_UNKNOWN);
        assert!(!result.is_known);
    }

    #[test]
    fn test_unknown_token_different_address() {
        // Unknown: network 0 with non-zero address
        let mut token = [0u8; 20];
        token[19] = 0x01; // Non-zero address
        let result = lookup_asset(0, &token);
        assert_eq!(result.symbol, SYMBOL_UNKNOWN);
        assert!(!result.is_known);
    }

    #[test]
    fn test_unknown_token_both_different() {
        // Unknown: non-zero network with non-zero address
        let token = [0xABu8; 20];
        let result = lookup_asset(5, &token);
        assert_eq!(result.symbol, SYMBOL_UNKNOWN);
        assert!(!result.is_known);
    }

    #[test]
    fn test_lumia_token_address() {
        // LUMIA token on Ethereum - should be UNKNOWN for now
        // Address: 0xD9343a049D5DBd89CD19DC6BcA8c48fB3a0a42a7
        let lumia_token: [u8; 20] = [
            0xD9, 0x34, 0x3a, 0x04, 0x9D, 0x5D, 0xBd, 0x89, 0xCD, 0x19,
            0xDC, 0x6B, 0xCA, 0x8c, 0x48, 0xfB, 0x3a, 0x0a, 0x42, 0xa7,
        ];
        let result = lookup_asset(0, &lumia_token);
        // LUMIA is not yet in the mapping - should be UNKNOWN
        assert_eq!(result.symbol, SYMBOL_UNKNOWN);
        assert!(!result.is_known);
    }
}
