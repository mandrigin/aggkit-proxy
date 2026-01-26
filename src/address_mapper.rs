//! Address mapping between Ethereum and Miden addresses.
//!
//! Implements "Option 3: Auto-create accounts" from the spec:
//! - When a claim arrives for an unknown Ethereum address, auto-create a Miden wallet
//! - Use deterministic derivation: eth_address -> keccak256 -> seed -> AccountId
//!
//! AccountId Structure:
//! - 15 bytes total (120 bits)
//! - Bytes 0-7: Prefix (Felt) - contains metadata in least significant byte
//! - Bytes 8-14: Suffix (Felt, 7 bytes since LSB is always 0)
//! - Byte 7 metadata: bits 0-3=version, bits 4-5=type, bits 6-7=storage_mode

use crate::error::ProxyError;
use crate::storage::{AddressMapping, MappingStorage};
use miden_protocol::account::{AccountId, AccountStorageMode, AccountType};
#[cfg(test)]
use miden_protocol::account::AccountIdVersion;
use sha3::{Digest, Keccak256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Result type for address mapper operations.
pub type Result<T> = std::result::Result<T, ProxyError>;

/// Configuration for the AddressMapper.
#[derive(Debug, Clone)]
pub struct AddressMapperConfig {
    /// Domain separator for deterministic derivation to avoid collisions
    pub domain_separator: Vec<u8>,
}

impl Default for AddressMapperConfig {
    fn default() -> Self {
        Self {
            domain_separator: b"aggkit-proxy-eth-to-miden-v1".to_vec(),
        }
    }
}

/// Maps Ethereum addresses (160-bit) to Miden AccountIds (120-bit).
///
/// Uses SQLite for persistent storage and supports:
/// - Explicit registration of address mappings
/// - Auto-creation of Miden accounts for unknown addresses
/// - Deterministic derivation of seeds from Ethereum addresses
pub struct AddressMapper {
    storage: MappingStorage,
    config: AddressMapperConfig,
}

impl AddressMapper {
    /// Create a new AddressMapper with SQLite storage at the given path.
    pub fn new<P: AsRef<Path>>(db_path: P, config: AddressMapperConfig) -> Result<Self> {
        let storage = MappingStorage::new(db_path)?;
        Ok(Self { storage, config })
    }

    /// Create an in-memory AddressMapper (useful for testing).
    pub fn in_memory(config: AddressMapperConfig) -> Result<Self> {
        let storage = MappingStorage::in_memory()?;
        Ok(Self { storage, config })
    }

    /// Look up a Miden AccountId for an Ethereum address.
    ///
    /// Returns `None` if no mapping exists.
    pub fn lookup(&self, eth_address: &EthAddress) -> Result<Option<MidenAccountId>> {
        if let Some(mapping) = self.storage.get_by_eth_address(&eth_address.0)? {
            Ok(Some(MidenAccountId::from_slice(&mapping.miden_account_id)?))
        } else {
            Ok(None)
        }
    }

    /// Get or create a Miden AccountId for an Ethereum address.
    ///
    /// **Simple encoding**: If the Eth address starts with 5 zero bytes (our encoding),
    /// the Miden AccountId is extracted directly by stripping those zeros.
    /// This allows users to encode their Miden address as: `0x0000000000<miden_addr>`
    ///
    /// **Legacy/fallback**: If the address doesn't use simple encoding and no mapping
    /// exists, derives a seed deterministically and creates a new Miden wallet account.
    ///
    /// Returns `(account_id, was_created)` where `was_created` is true if a new
    /// account was auto-created (only for legacy derivation, not simple encoding).
    pub fn get_or_create(&self, eth_address: &EthAddress) -> Result<(MidenAccountId, bool)> {
        // First, check if this is a simple zero-padded encoding
        if eth_address.is_miden_encoded() {
            let miden_account_id = eth_address.to_miden_simple()?;
            info!(
                eth = %eth_address,
                miden = %miden_account_id,
                "Using simple zero-padded encoding (Miden address embedded in Eth address)"
            );
            return Ok((miden_account_id, false));
        }

        // Check if mapping already exists in storage
        if let Some(mapping) = self.storage.get_by_eth_address(&eth_address.0)? {
            let miden_account_id = MidenAccountId::from_slice(&mapping.miden_account_id)?;
            debug!(
                eth = %eth_address,
                miden = %miden_account_id,
                "Found existing mapping in storage"
            );
            return Ok((miden_account_id, false));
        }

        // Fallback: Derive seed deterministically from Ethereum address
        let seed = self.derive_seed(eth_address);
        info!(
            eth = %eth_address,
            "Deriving new Miden account from Ethereum address (legacy mode)"
        );

        // Create the Miden account with proper metadata bits
        // This creates a RegularAccountUpdatableCode with Public storage mode
        let miden_account_id = self.seed_to_account_id(&seed)?;

        // Store the mapping (convert AccountId to bytes for storage)
        let mapping = AddressMapping {
            eth_address: eth_address.0,
            miden_account_id: miden_account_id.to_bytes(),
            created_at: current_timestamp(),
            auto_created: true,
        };
        self.storage.insert(&mapping)?;

        info!(
            eth = %eth_address,
            miden = %miden_account_id,
            account_type = ?miden_account_id.inner().account_type(),
            storage_mode = ?miden_account_id.inner().storage_mode(),
            "Auto-created new Miden account mapping (legacy derivation)"
        );

        Ok((miden_account_id, true))
    }

    /// Register an explicit mapping between an Ethereum address and Miden AccountId.
    ///
    /// This is used when the user already has a Miden account and wants to link it
    /// to their Ethereum address.
    pub fn register(
        &self,
        eth_address: &EthAddress,
        miden_account_id: &MidenAccountId,
    ) -> Result<()> {
        if self.storage.exists(&eth_address.0)? {
            warn!(
                eth = %eth_address,
                "Attempted to register address that already has a mapping"
            );
            return Err(ProxyError::AccountResolution {
                eth_address: eth_address.to_string(),
                reason: "address already has a mapping".to_string(),
            });
        }

        let mapping = AddressMapping {
            eth_address: eth_address.0,
            miden_account_id: miden_account_id.to_bytes(),
            created_at: current_timestamp(),
            auto_created: false,
        };
        self.storage.insert(&mapping)?;

        info!(
            eth = %eth_address,
            miden = %miden_account_id,
            "Registered explicit address mapping"
        );

        Ok(())
    }

    /// Reverse lookup: find the Ethereum address for a Miden AccountId.
    pub fn reverse_lookup(&self, miden_id: &MidenAccountId) -> Result<Option<EthAddress>> {
        let miden_bytes = miden_id.to_bytes();
        if let Some(mapping) = self.storage.get_by_miden_id(&miden_bytes)? {
            Ok(Some(EthAddress(mapping.eth_address)))
        } else {
            Ok(None)
        }
    }

    /// Derive a seed deterministically from an Ethereum address.
    ///
    /// Uses keccak256 with a domain separator to avoid collisions with other
    /// derivation schemes.
    pub fn derive_seed(&self, eth_address: &EthAddress) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(&self.config.domain_separator);
        hasher.update(&eth_address.0);
        hasher.finalize().into()
    }

    /// Convert a 32-byte seed to a proper Miden AccountId with correct metadata bits.
    ///
    /// Creates a `RegularAccountUpdatableCode` with `Public` storage mode.
    ///
    /// The AccountId is 15 bytes with metadata encoded in byte 7 (the LSB of the prefix Felt):
    /// - bits 0-3: version (0 for V0)
    /// - bits 4-5: account type (0=RegularUpdatable, 1=RegularImmutable, 2=FungibleFaucet, 3=NonFungibleFaucet)
    /// - bits 6-7: storage mode (0=Public, 1=Network, 2=Private)
    ///
    /// The AccountId must satisfy these constraints:
    /// - Prefix (bytes 0-7): bit 32 must be 0 (for valid Felt)
    /// - Suffix (bytes 8-14): MSB must be 0, LSB must be 0
    fn seed_to_account_id(&self, seed: &[u8; 32]) -> Result<MidenAccountId> {
        // Hash the seed to get deterministic bytes for the AccountId
        // This ensures we have a clean separation between the seed and the ID
        let mut hasher = Keccak256::new();
        hasher.update(b"miden-account-id-v1");
        hasher.update(seed);
        let hash = hasher.finalize();

        // Take the first 15 bytes for the 120-bit AccountId
        let mut account_id_bytes = [0u8; 15];
        account_id_bytes.copy_from_slice(&hash[..15]);

        // Set proper metadata bits in byte 7 (the LSB of the prefix Felt)
        // For RegularAccountUpdatableCode (type=0) + Public storage (mode=0) + Version 0:
        // low_nibble = (storage_mode << 6) | (account_type << 4) | version
        //            = (0 << 6) | (0 << 4) | 0 = 0x00
        let account_type = AccountType::RegularAccountUpdatableCode;
        let storage_mode = AccountStorageMode::Public;
        let version: u8 = 0; // AccountIdVersion::Version0

        let metadata_byte = ((storage_mode as u8) << 6) | ((account_type as u8) << 4) | version;
        account_id_bytes[7] = metadata_byte;

        // Clear the 32nd most significant bit of prefix (bit 0 of byte 3)
        // This ensures the prefix is a valid Felt (< field modulus)
        account_id_bytes[3] &= 0b1111_1110;

        // Clear the MSB of the suffix (bit 7 of byte 8)
        // The suffix's most significant bit must be zero
        account_id_bytes[8] &= 0b0111_1111;

        // The suffix's LSB is handled in the 15->Felt conversion
        // (last byte is byte 14, and Felt expects lower 8 bits to be 0 after conversion)

        // Create the AccountId from the shaped bytes
        let account_id = AccountId::try_from(account_id_bytes).map_err(|e| {
            ProxyError::AccountResolution {
                eth_address: "unknown".to_string(),
                reason: format!("failed to create AccountId from seed: {}", e),
            }
        })?;

        // Verify the account type and storage mode are correct
        debug_assert_eq!(account_id.account_type(), account_type);
        debug_assert_eq!(account_id.storage_mode(), storage_mode);

        Ok(MidenAccountId(account_id))
    }

    /// Get total number of mappings stored.
    pub fn count(&self) -> Result<u64> {
        self.storage.count()
    }
}

/// Ethereum address (160-bit, 20 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EthAddress(pub [u8; 20]);

/// Number of padding bytes when encoding Miden AccountId as Eth address.
/// Eth = 20 bytes, Miden = 15 bytes, so 5 bytes of padding.
pub const MIDEN_ETH_PADDING_BYTES: usize = 5;

impl EthAddress {
    /// Create from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 20 {
            return Err(ProxyError::AccountResolution {
                eth_address: hex::encode(bytes),
                reason: format!("invalid address length: expected 20 bytes, got {}", bytes.len()),
            });
        }
        let mut addr = [0u8; 20];
        addr.copy_from_slice(bytes);
        Ok(Self(addr))
    }

    /// Create from a hex string (with or without 0x prefix).
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let bytes = hex::decode(hex_str).map_err(|e| ProxyError::AccountResolution {
            eth_address: hex_str.to_string(),
            reason: format!("invalid hex: {}", e),
        })?;
        Self::from_slice(&bytes)
    }

    /// Create from an alloy Address type.
    pub fn from_alloy(addr: &alloy_primitives::Address) -> Self {
        Self(addr.0 .0)
    }

    /// Convert to an alloy Address type.
    pub fn to_alloy(&self) -> alloy_primitives::Address {
        alloy_primitives::Address::from_slice(&self.0)
    }

    /// Create an Eth address from a Miden AccountId using zero-padding.
    ///
    /// The encoding is: `eth_addr = 0x0000000000 || miden_addr`
    /// (5 zero bytes prefix + 15 bytes of Miden AccountId)
    ///
    /// This is the inverse of `to_miden_simple()`.
    pub fn from_miden_simple(miden_id: &MidenAccountId) -> Self {
        let mut eth_bytes = [0u8; 20];
        // First 5 bytes are zeros (padding)
        // Last 15 bytes are the Miden AccountId
        eth_bytes[MIDEN_ETH_PADDING_BYTES..].copy_from_slice(&miden_id.to_bytes());
        Self(eth_bytes)
    }

    /// Convert to a Miden AccountId using simple zero-stripping.
    ///
    /// Expects the first 5 bytes to be zeros (our encoding convention).
    /// Returns an error if the padding bytes are not zero.
    ///
    /// This is the inverse of `from_miden_simple()`.
    pub fn to_miden_simple(&self) -> Result<MidenAccountId> {
        // Verify the first 5 bytes are zeros
        let padding = &self.0[..MIDEN_ETH_PADDING_BYTES];
        if padding != [0u8; MIDEN_ETH_PADDING_BYTES] {
            return Err(ProxyError::AccountResolution {
                eth_address: self.to_string(),
                reason: format!(
                    "invalid padding: expected {} zero bytes, got {:?}",
                    MIDEN_ETH_PADDING_BYTES, padding
                ),
            });
        }

        // Take the last 15 bytes as the Miden AccountId
        let miden_bytes = &self.0[MIDEN_ETH_PADDING_BYTES..];
        MidenAccountId::from_slice(miden_bytes)
    }

    /// Check if this Eth address uses our simple Miden encoding (5 zero byte prefix).
    pub fn is_miden_encoded(&self) -> bool {
        self.0[..MIDEN_ETH_PADDING_BYTES] == [0u8; MIDEN_ETH_PADDING_BYTES]
    }
}

impl std::fmt::Display for EthAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl From<alloy_primitives::Address> for EthAddress {
    fn from(addr: alloy_primitives::Address) -> Self {
        Self::from_alloy(&addr)
    }
}

impl From<EthAddress> for alloy_primitives::Address {
    fn from(addr: EthAddress) -> Self {
        addr.to_alloy()
    }
}

/// Miden AccountId wrapper for address mapping.
///
/// Wraps the proper `miden_protocol::account::AccountId` type and provides
/// conversion utilities for storage and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MidenAccountId(pub AccountId);

impl MidenAccountId {
    /// Create from a byte slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 15 {
            return Err(ProxyError::AccountResolution {
                eth_address: "unknown".to_string(),
                reason: format!(
                    "invalid Miden AccountId: expected 15 bytes, got {}",
                    bytes.len()
                ),
            });
        }
        let mut id_bytes = [0u8; 15];
        id_bytes.copy_from_slice(bytes);

        let account_id = AccountId::try_from(id_bytes).map_err(|e| {
            ProxyError::AccountResolution {
                eth_address: "unknown".to_string(),
                reason: format!("invalid Miden AccountId bytes: {}", e),
            }
        })?;

        Ok(Self(account_id))
    }

    /// Convert to bytes for storage.
    pub fn to_bytes(&self) -> [u8; 15] {
        self.0.into()
    }

    /// Get the underlying AccountId.
    pub fn inner(&self) -> AccountId {
        self.0
    }

    /// Convert to an Eth address using zero-padding.
    ///
    /// The encoding is: `eth_addr = 0x0000000000 || miden_addr`
    /// (5 zero bytes prefix + 15 bytes of Miden AccountId)
    pub fn to_eth_padded(&self) -> EthAddress {
        EthAddress::from_miden_simple(self)
    }

    /// Create from an Eth address that uses our zero-padding encoding.
    ///
    /// Expects the Eth address to be in format: `0x0000000000<miden_addr>`
    pub fn from_eth_padded(eth_addr: &EthAddress) -> Result<Self> {
        eth_addr.to_miden_simple()
    }
}

impl std::fmt::Display for MidenAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<AccountId> for MidenAccountId {
    fn from(id: AccountId) -> Self {
        Self(id)
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AddressMapperConfig {
        AddressMapperConfig::default()
    }

    /// Create a valid test AccountId with proper metadata bits.
    /// Uses AccountId::dummy() which is available in test mode.
    fn create_test_account_id(bytes: [u8; 15]) -> MidenAccountId {
        // Use the testing feature's dummy() function to create valid AccountIds
        let account_id = AccountId::dummy(
            bytes,
            AccountIdVersion::Version0,
            AccountType::RegularAccountUpdatableCode,
            AccountStorageMode::Public,
        );
        MidenAccountId(account_id)
    }

    #[test]
    fn test_get_or_create_new_address() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([1u8; 20]);
        let (miden_id, was_created) = mapper.get_or_create(&eth_addr).unwrap();

        assert!(was_created);
        assert_eq!(mapper.count().unwrap(), 1);

        // Verify the account has correct type and storage mode
        assert_eq!(miden_id.inner().account_type(), AccountType::RegularAccountUpdatableCode);
        assert_eq!(miden_id.inner().storage_mode(), AccountStorageMode::Public);

        // Second call should return the same ID
        let (miden_id2, was_created2) = mapper.get_or_create(&eth_addr).unwrap();
        assert!(!was_created2);
        assert_eq!(miden_id, miden_id2);
    }

    #[test]
    fn test_deterministic_derivation() {
        let mapper1 = AddressMapper::in_memory(test_config()).unwrap();
        let mapper2 = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([42u8; 20]);

        // Both mappers should derive the same seed and account ID
        let seed1 = mapper1.derive_seed(&eth_addr);
        let seed2 = mapper2.derive_seed(&eth_addr);
        assert_eq!(seed1, seed2);

        let (id1, _) = mapper1.get_or_create(&eth_addr).unwrap();
        let (id2, _) = mapper2.get_or_create(&eth_addr).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_explicit_registration() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([5u8; 20]);
        // Create a valid test AccountId with proper metadata
        let miden_id = create_test_account_id([6u8; 15]);

        mapper.register(&eth_addr, &miden_id).unwrap();

        let retrieved = mapper.lookup(&eth_addr).unwrap();
        assert_eq!(retrieved, Some(miden_id));
    }

    #[test]
    fn test_reverse_lookup() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([7u8; 20]);
        let (miden_id, _) = mapper.get_or_create(&eth_addr).unwrap();

        let reverse = mapper.reverse_lookup(&miden_id).unwrap();
        assert_eq!(reverse, Some(eth_addr));
    }

    #[test]
    fn test_different_addresses_different_ids() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth1 = EthAddress([1u8; 20]);
        let eth2 = EthAddress([2u8; 20]);

        let (id1, _) = mapper.get_or_create(&eth1).unwrap();
        let (id2, _) = mapper.get_or_create(&eth2).unwrap();

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_account_id_metadata() {
        // Test that derived AccountIds have correct metadata
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        let eth_addr = EthAddress([0xde; 20]);
        let (miden_id, _) = mapper.get_or_create(&eth_addr).unwrap();

        // Verify the AccountId has the expected type and storage mode
        assert_eq!(
            miden_id.inner().account_type(),
            AccountType::RegularAccountUpdatableCode,
            "AccountId should be RegularAccountUpdatableCode"
        );
        assert_eq!(
            miden_id.inner().storage_mode(),
            AccountStorageMode::Public,
            "AccountId should have Public storage mode"
        );
    }

    #[test]
    fn test_from_hex() {
        let addr = EthAddress::from_hex("0x742d35Cc6634C0532925a3b844Bc9e7595f41111").unwrap();
        assert_eq!(addr.0[0], 0x74);
        assert_eq!(addr.0[19], 0x11);

        // Without 0x prefix
        let addr2 = EthAddress::from_hex("742d35Cc6634C0532925a3b844Bc9e7595f41111").unwrap();
        assert_eq!(addr, addr2);
    }

    #[test]
    fn test_alloy_conversion() {
        let alloy_addr =
            alloy_primitives::Address::from_slice(&[0xab; 20]);
        let eth_addr = EthAddress::from_alloy(&alloy_addr);
        assert_eq!(eth_addr.0, [0xab; 20]);

        let back = eth_addr.to_alloy();
        assert_eq!(back, alloy_addr);
    }

    #[test]
    fn test_simple_miden_encoding_roundtrip() {
        // Create a test Miden AccountId
        let miden_id = create_test_account_id([0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
                                               0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        // Convert to Eth address (should add 5 zero bytes prefix)
        let eth_addr = EthAddress::from_miden_simple(&miden_id);

        // Verify the first 5 bytes are zeros
        assert_eq!(&eth_addr.0[..5], &[0u8; 5]);
        assert!(eth_addr.is_miden_encoded());

        // Convert back to Miden (should strip the zeros)
        let miden_back = eth_addr.to_miden_simple().unwrap();
        assert_eq!(miden_id.to_bytes(), miden_back.to_bytes());
    }

    #[test]
    fn test_simple_encoding_in_get_or_create() {
        let mapper = AddressMapper::in_memory(test_config()).unwrap();

        // Create a Miden AccountId and encode as Eth address
        let miden_id = create_test_account_id([0xaa; 15]);
        let eth_addr = miden_id.to_eth_padded();

        // get_or_create should recognize the simple encoding and return the same ID
        let (retrieved_id, was_created) = mapper.get_or_create(&eth_addr).unwrap();

        // Should NOT be marked as created (simple encoding doesn't create new accounts)
        assert!(!was_created);
        assert_eq!(miden_id.to_bytes(), retrieved_id.to_bytes());
    }

    #[test]
    fn test_non_miden_encoded_address() {
        // An address that doesn't start with 5 zero bytes
        let eth_addr = EthAddress([0x11; 20]);
        assert!(!eth_addr.is_miden_encoded());

        // to_miden_simple should fail
        let result = eth_addr.to_miden_simple();
        assert!(result.is_err());
    }
}
